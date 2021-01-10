use crate::ast::*;
use crate::buckets::*;
use crate::filedb::*;
use crate::tc_ast::*;
use crate::util::*;
use std::collections::HashSet;
use std::mem::Discriminant;

type BuiltinTransform = for<'a, 'b, 'c, 'd, 'e> fn(
    CheckEnv<'a, 'b>,
    &'c LocalTypeEnv,
    CodeLoc,
    &'d [Expr<'e>],
) -> Result<(TCBuiltin<'b>, TCType), Error>;

lazy_static! {
    pub static ref BUILTINS: HashMap<&'static str, BuiltinTransform> = {
        let mut m: HashMap<&'static str, BuiltinTransform> = HashMap::new();

        m.insert(
            "__tci_builtin_push_temp_stack",
            |env, local_env, call_loc, args| {
                if args.len() != 2 {
                    return Err(error!(
                        "wrong number of arguments to builtin function",
                        call_loc, "called here"
                    ));
                }

                let ptr = check_expr(env, local_env, &args[0])?;
                let ptr = env.assign_convert(&TCType::new(TCTypeKind::Void, 1), call_loc, ptr)?;

                let size = check_expr(env, local_env, &args[1])?;
                let size = env.assign_convert(&TCType::new(TCTypeKind::U32, 0), call_loc, size)?;

                return Ok((
                    TCBuiltin::PushTempStack {
                        ptr: &*env.buckets.add(ptr),
                        size: &*env.buckets.add(size),
                    },
                    TCType::new(TCTypeKind::Void, 0),
                ));
            },
        );

        m.insert("__tci_builtin_ecall", |env, local_env, call_loc, args| {
            if args.len() != 1 {
                return Err(error!(
                    "wrong number of arguments to builtin function",
                    call_loc, "called here"
                ));
            }

            let ecall = check_expr(env, local_env, &args[0])?;
            let ecall = env.assign_convert(&TCType::new(TCTypeKind::U32, 0), call_loc, ecall)?;

            let size_t = TCType::new(TCTypeKind::U64, 0);
            return Ok((TCBuiltin::Ecall(&*env.buckets.add(ecall)), size_t));
        });

        m
    };
}

pub fn unify<'a>(
    env: CheckEnv<'_, 'a>,
    mut l: TCExpr<'a>,
    mut r: TCExpr<'a>,
) -> Result<(TCExpr<'a>, TCExpr<'a>, TCPrimType), Error> {
    l.expr_type = env.resolve_typedef(l.expr_type, l.loc)?; // TODO why do we mutate here?
    r.expr_type = env.resolve_typedef(r.expr_type, r.loc)?;

    let l_prim = env.to_prim_type(l.expr_type, l.loc)?;
    let r_prim = env.to_prim_type(r.expr_type, r.loc)?;

    // TODO pointer unification unifies to void*

    if l_prim == r_prim {
        return Ok((l, r, l_prim));
    }

    let (l_rank, r_rank) = (l.expr_type.rank(), r.expr_type.rank());
    if l_rank == 0 || r_rank == 0 {
        return Err(error!(
            "can't unify these two types",
            l.loc,
            format!("this has type {}", l.expr_type.display(env.files)),
            r.loc,
            format!("this has type {}", r.expr_type.display(env.files))
        ));
    }

    if l_rank > r_rank {
        let r = TCExpr {
            kind: TCExprKind::Conv {
                from: r_prim,
                to: l_prim,
                expr: env.buckets.add(r),
            },
            expr_type: l.expr_type,
            loc: r.loc,
        };
        return Ok((l, r, l_prim));
    } else {
        let l = TCExpr {
            kind: TCExprKind::Conv {
                from: l_prim,
                to: r_prim,
                expr: env.buckets.add(l),
            },
            expr_type: r.expr_type,
            loc: r.loc,
        };
        return Ok((l, r, r_prim));
    }
}

type BinOpTransform =
    for<'b> fn(CheckEnv<'_, 'b>, TCExpr<'b>, TCExpr<'b>) -> Result<TCExpr<'b>, Error>;
type UnOpTransform = for<'b> fn(BucketListRef<'b>, TCExpr<'b>, CodeLoc) -> TCExpr<'b>;
type Transform = for<'b> fn(BucketListRef<'b>, TCExpr<'b>, TCType) -> TCExpr<'b>;

// Implicit Transforms
pub type TCPrimDiscr = Discriminant<TCPrimType>;
pub type BinOpOverloads = HashMap<(BinOp, TCPrimDiscr, TCPrimDiscr), BinOpTransform>;
pub type UnOpOverloads = HashMap<(UnaryOp, TCPrimDiscr), UnOpTransform>;

pub struct Overloads {
    pub unary_op: UnOpOverloads,
    pub bin_op: BinOpOverloads,
}

lazy_static! {
    pub static ref OVERLOADS: Overloads = {
        let mut bin_op: BinOpOverloads = HashMap::new();
        let mut unary_op: UnOpOverloads = HashMap::new();

        // primitive type discriminant (get discriminant of enum TCPrimType)
        macro_rules! pt_discr {
            (Pointer) => {{
                TCPrimType::Pointer { stride_length: 0 }.discriminant()
            }};
            ($id:ident) => {{
                TCPrimType::$id.discriminant()
            }};
        }

        macro_rules! add_un_op_ol {
            ($op:ident, $operand:ident, $func:expr) => {{
                unary_op.insert((UnaryOp::$op, pt_discr!($operand)), $func);
            }};
        }

        add_un_op_ol!(Neg, I32, |buckets, op, loc| {
            let result_type = TCType::new(TCTypeKind::I32, 0);
            let negative_one = TCExpr {
                loc,
                kind: TCExprKind::I32Literal(-1),
                expr_type: result_type,
            };
            return TCExpr {
                loc,
                kind: TCExprKind::BinOp {
                    op: BinOp::Mul,
                    op_type: TCPrimType::I32,
                    left: buckets.add(negative_one),
                    right: buckets.add(op),
                },
                expr_type: result_type,
            };
        });

        macro_rules! add_op_ol {
            ($op:ident, $left:ident, $right:ident, $func:expr) => {{
                bin_op.insert((BinOp::$op, pt_discr!($left), pt_discr!($right)), $func);
            }};
        }

        add_op_ol!(RShift, I32, I32, |env, l, r| {
            let r = env.assign_convert(&TCType::new(TCTypeKind::I8, 0), NO_FILE, r)?;
            return Ok(TCExpr {
                loc: l_from(l.loc, r.loc),
                expr_type: l.expr_type,
                kind: TCExprKind::BinOp {
                    op: BinOp::RShift,
                    op_type: TCPrimType::I32,
                    left: env.buckets.add(l),
                    right: env.buckets.add(r),
                },
            });
        });

        add_op_ol!(LShift, I32, I32, |env, l, r| {
            let r = env.assign_convert(&TCType::new(TCTypeKind::I8, 0), NO_FILE, r)?;
            return Ok(TCExpr {
                loc: l_from(l.loc, r.loc),
                expr_type: l.expr_type,
                kind: TCExprKind::BinOp {
                    op: BinOp::LShift,
                    op_type: TCPrimType::I32,
                    left: env.buckets.add(l),
                    right: env.buckets.add(r),
                },
            });
        });

        macro_rules! pointer_add_ol {
            (@UNSIGNED, $op_simp:ident, $ty:ident) => {{
                add_op_ol!($op_simp, Pointer, $ty, |env, l, r| {
                    let elem_type = env.deref(l.expr_type, l.loc)?; // should this ever fail?

                    let r = TCExpr {
                        loc: r.loc,
                        kind: TCExprKind::Conv {
                            from: TCPrimType::$ty,
                            to: TCPrimType::U64,
                            expr: env.buckets.add(r),
                        },
                        expr_type: TCType::new(TCTypeKind::U64, 0),
                    };

                    let size_of_elements = TCExpr {
                        loc: l.loc,
                        kind: TCExprKind::U64Literal(elem_type.size() as u64),
                        expr_type: TCType::new(TCTypeKind::U64, 0),
                    };

                    let r = TCExpr {
                        loc: r.loc,
                        kind: TCExprKind::BinOp {
                            op: BinOp::Mul,
                            op_type: TCPrimType::U64,
                            left: env.buckets.add(r),
                            right: env.buckets.add(size_of_elements),
                        },
                        expr_type: TCType::new(TCTypeKind::U64, 0),
                    };

                    return Ok(TCExpr {
                        loc: l_from(l.loc, r.loc),
                        expr_type: l.expr_type,
                        kind: TCExprKind::BinOp {
                            op: BinOp::$op_simp,
                            op_type: TCPrimType::Pointer {
                                stride_length: elem_type.size(),
                            },
                            left: env.buckets.add(l),
                            right: env.buckets.add(r),
                        },
                    });
                });
            }};
            (@SIGNED, $op_simp:ident, $ty:ident) => {{
                add_op_ol!($op_simp, Pointer, $ty, |env, l, r| {
                    let elem_type = env.deref(l.expr_type, l.loc)?; // should this ever fail?

                    let r = TCExpr {
                        loc: r.loc,
                        kind: TCExprKind::Conv {
                            from: TCPrimType::$ty,
                            to: TCPrimType::I64,
                            expr: env.buckets.add(r),
                        },
                        expr_type: TCType::new(TCTypeKind::I64, 0),
                    };

                    let size_of_elements = TCExpr {
                        loc: l.loc,
                        kind: TCExprKind::I64Literal(elem_type.size() as i64),
                        expr_type: TCType::new(TCTypeKind::I64, 0),
                    };

                    let r = TCExpr {
                        loc: r.loc,
                        kind: TCExprKind::BinOp {
                            op: BinOp::Mul,
                            op_type: TCPrimType::I64,
                            left: env.buckets.add(r),
                            right: env.buckets.add(size_of_elements),
                        },
                        expr_type: TCType::new(TCTypeKind::I64, 0),
                    };

                    return Ok(TCExpr {
                        loc: l_from(l.loc, r.loc),
                        expr_type: l.expr_type,
                        kind: TCExprKind::BinOp {
                            op: BinOp::$op_simp,
                            op_type: TCPrimType::Pointer {
                                stride_length: elem_type.size(),
                            },
                            left: env.buckets.add(l),
                            right: env.buckets.add(r),
                        },
                    });
                });
            }};
        }

        pointer_add_ol!(@SIGNED, Add, I32);
        pointer_add_ol!(@SIGNED, Add, I64);
        pointer_add_ol!(@UNSIGNED, Add, U64);
        pointer_add_ol!(@UNSIGNED, Sub, U64);

        macro_rules! pointer_add_index_ol {
            (@UNSIGNED, $ty:ident) => {{
                add_op_ol!(Index, Pointer, $ty, |env, l, r| {
                    let elem_type = env.deref(l.expr_type, l.loc)?; // should this ever fail?

                    let r = TCExpr {
                        loc: r.loc,
                        kind: TCExprKind::Conv {
                            from: TCPrimType::$ty,
                            to: TCPrimType::U64,
                            expr: env.buckets.add(r),
                        },
                        expr_type: TCType::new(TCTypeKind::U64, 0),
                    };

                    let size_of_elements = TCExpr {
                        loc: l.loc,
                        kind: TCExprKind::U64Literal(elem_type.size() as u64),
                        expr_type: TCType::new(TCTypeKind::U64, 0),
                    };

                    let r = TCExpr {
                        loc: r.loc,
                        kind: TCExprKind::BinOp {
                            op: BinOp::Mul,
                            op_type: TCPrimType::U64,
                            left: env.buckets.add(r),
                            right: env.buckets.add(size_of_elements),
                        },
                        expr_type: TCType::new(TCTypeKind::U64, 0),
                    };

                    let ptr = TCExpr {
                        loc: l_from(l.loc, r.loc),
                        expr_type: l.expr_type,
                        kind: TCExprKind::BinOp {
                            op: BinOp::Add,
                            op_type: TCPrimType::Pointer {
                                stride_length: elem_type.size(),
                            },
                            left: env.buckets.add(l),
                            right: env.buckets.add(r),
                        },
                    };

                    return Ok(TCExpr {
                        kind: TCExprKind::Deref(env.buckets.add(ptr)),
                        loc: ptr.loc,
                        expr_type: elem_type,
                    });
                });
            }};
            (@SIGNED, $ty:ident) => {{
                add_op_ol!(Index, Pointer, $ty, |env, l, r| {
                    let elem_type = env.deref(l.expr_type, l.loc)?; // should this ever fail?

                    let r = TCExpr {
                        loc: r.loc,
                        kind: TCExprKind::Conv {
                            from: TCPrimType::$ty,
                            to: TCPrimType::I64,
                            expr: env.buckets.add(r),
                        },
                        expr_type: TCType::new(TCTypeKind::I64, 0),
                    };

                    let size_of_elements = TCExpr {
                        loc: l.loc,
                        kind: TCExprKind::I64Literal(elem_type.size() as i64),
                        expr_type: TCType::new(TCTypeKind::I64, 0),
                    };

                    let r = TCExpr {
                        loc: r.loc,
                        kind: TCExprKind::BinOp {
                            op: BinOp::Mul,
                            op_type: TCPrimType::I64,
                            left: env.buckets.add(r),
                            right: env.buckets.add(size_of_elements),
                        },
                        expr_type: TCType::new(TCTypeKind::I64, 0),
                    };

                    let ptr = TCExpr {
                        loc: l_from(l.loc, r.loc),
                        expr_type: l.expr_type,
                        kind: TCExprKind::BinOp {
                            op: BinOp::Add,
                            op_type: TCPrimType::Pointer {
                                stride_length: elem_type.size(),
                            },
                            left: env.buckets.add(l),
                            right: env.buckets.add(r),
                        },
                    };

                    return Ok(TCExpr {
                        kind: TCExprKind::Deref(env.buckets.add(ptr)),
                        loc: ptr.loc,
                        expr_type: elem_type,
                    });
                });
            }};
        }

        pointer_add_index_ol!(@SIGNED, I32);
        pointer_add_index_ol!(@SIGNED, I64);
        pointer_add_index_ol!(@UNSIGNED, U32);
        pointer_add_index_ol!(@UNSIGNED, U64);

        add_op_ol!(Sub, Pointer, Pointer, |env, l, r| {
            let l_elem_type = env.deref(l.expr_type, l.loc).unwrap();
            let r_elem_type = env.deref(r.expr_type, r.loc).unwrap();
            if !env.type_eq(l_elem_type, r_elem_type) {
                return Err(error!(
                    "pointers aren't the same type",
                    l.loc,
                    format!("this has type {}", l.expr_type.display(env.files)),
                    r.loc,
                    format!("this has type {}", r.expr_type.display(env.files))
                ));
            }

            let expr_type = TCType::new(TCTypeKind::U64, 0);
            let loc = l_from(l.loc, r.loc);
            let diff = TCExpr {
                kind: TCExprKind::BinOp {
                    op: BinOp::Sub,
                    op_type: TCPrimType::U64,
                    left: env.buckets.add(l),
                    right: env.buckets.add(r),
                },
                loc,
                expr_type,
            };

            let divisor = TCExpr {
                kind: TCExprKind::U64Literal(l_elem_type.size() as u64),
                expr_type,
                loc,
            };

            return Ok(TCExpr {
                kind: TCExprKind::BinOp {
                    op: BinOp::Div,
                    op_type: TCPrimType::U64,
                    left: env.buckets.add(diff),
                    right: env.buckets.add(divisor),
                },
                loc,
                expr_type,
            });
        });

        Overloads { unary_op, bin_op }
    };
}

fn get_overload(
    env: CheckEnv,
    op: BinOp,
    l: &TCExpr,
    r: &TCExpr,
) -> Result<Option<BinOpTransform>, Error> {
    let l_et = env.resolve_typedef(l.expr_type, l.loc)?;
    let r_et = env.resolve_typedef(r.expr_type, r.loc)?;

    let key = (
        op,
        env.to_prim_type(l_et, l.loc)?.discriminant(),
        env.to_prim_type(r_et, r.loc)?.discriminant(),
    );
    match OVERLOADS.bin_op.get(&key) {
        Some(bin_op) => return Ok(Some(*bin_op)),
        None => return Ok(None),
    }
}

pub struct LocalTypeEnv {
    pub symbols: HashMap<u32, TCVar>,
    pub return_type: TCType,
    pub rtype_loc: CodeLoc,
    pub parent: *const LocalTypeEnv,
    pub decl_idx: i16,
}

impl LocalTypeEnv {
    pub fn new(return_type: TCType, rtype_loc: CodeLoc) -> Self {
        Self {
            symbols: HashMap::new(),
            return_type,
            rtype_loc,
            parent: core::ptr::null(),
            decl_idx: 0,
        }
    }

    pub fn child(&self) -> Self {
        if self.symbols.is_empty() {
            // for the case of chained if-else
            Self {
                symbols: HashMap::new(),
                return_type: self.return_type,
                rtype_loc: self.rtype_loc,
                parent: self.parent,
                decl_idx: self.decl_idx,
            }
        } else {
            Self {
                symbols: HashMap::new(),
                return_type: self.return_type,
                rtype_loc: self.rtype_loc,
                decl_idx: self.decl_idx,
                parent: self,
            }
        }
    }

    pub fn var(&self, id: u32) -> Option<&TCVar> {
        if let Some(var_type) = self.symbols.get(&id) {
            return Some(var_type);
        }

        if self.parent.is_null() {
            return None;
        }

        return unsafe { &*self.parent }.var(id);
    }

    pub fn add_var(&mut self, ident: u32, tc_value: TCVar) -> Result<(), Error> {
        let tc_loc = tc_value.loc;
        if let Some(var_type) = self.symbols.insert(ident, tc_value) {
            return Err(error!(
                "name redefined in scope",
                var_type.loc, "first declaration defined here", tc_loc, "redecaration defined here"
            ));
        }

        return Ok(());
    }

    pub fn add_local(&mut self, ident: u32, decl_type: TCType, loc: CodeLoc) -> Result<(), Error> {
        let tc_var = TCVar {
            decl_type,
            var_offset: self.decl_idx,
            loc,
        };

        self.decl_idx += 1;

        return self.add_var(ident, tc_var);
    }
}

#[derive(Debug)]
pub struct TypeEnv {
    pub structs: HashMap<u32, TCStruct>,
    pub anon_structs: HashMap<CodeLoc, TCStruct>,
    pub typedefs: HashMap<u32, TCTypedef>,
    pub builtins_enabled: Option<u32>,
}

impl TypeEnv {
    pub fn new(builtins_enabled: Option<u32>) -> Self {
        Self {
            structs: HashMap::new(),
            anon_structs: HashMap::new(),
            typedefs: HashMap::new(),
            builtins_enabled,
        }
    }

    pub fn type_eq(&self, l: TCType, r: TCType) -> bool {
        // TODO actually make this do type equality
        return l == r;
    }

    pub fn deref(
        &self,
        files: &FileDb,
        tc_type: TCType,
        value_loc: CodeLoc,
    ) -> Result<TCType, Error> {
        let tc_type = self.resolve_typedef(tc_type, value_loc)?;
        if tc_type.pointer_count == 0 && tc_type.array_kind == TCArrayKind::None {
            return Err(error!(
                "cannot dereference values that aren't pointers",
                value_loc,
                format!(
                    "value has type {}, which cannot be dereferenced",
                    tc_type.display(files)
                )
            ));
        }

        let result_type = match tc_type.array_kind {
            TCArrayKind::None => TCType::new(tc_type.kind, tc_type.pointer_count - 1),
            TCArrayKind::Fixed(_) => TCType::new(tc_type.kind, tc_type.pointer_count),
        };

        if result_type.pointer_count > 0 {
            return Ok(result_type);
        }

        if let TCTypeKind::Struct {
            sa: TC_UNKNOWN_SA, ..
        } = result_type.kind
        {
            return Err(error!(
                "cannot dereference pointer to struct of unknown size",
                value_loc,
                format!(
                    "value has type {}, which cannot be dereferenced",
                    tc_type.display(files)
                )
            ));
        }

        return Ok(result_type);
    }

    /// Used to check the return type of functions
    pub fn check_func_itype(
        &self,
        decl_idx: u32,
        itype: IType,
        loc: CodeLoc,
    ) -> Result<TCType, Error> {
        let mut tc_type = itype.into();
        match &mut tc_type.kind {
            TCTypeKind::Struct { ident, sa } => {
                *sa = self.check_struct_type(*ident, decl_idx, tc_type.pointer_count, loc)?;
            }
            TCTypeKind::AnonStruct { loc, sa } => {}
            TCTypeKind::Ident { ident, sa: i_sa } => {
                let map_err = || typedef_not_defined(loc);
                let typedef = self.typedefs.get(&ident).ok_or_else(map_err)?;

                if typedef.defn_idx > decl_idx {
                    return Err(typedef_defined_later(typedef.loc, loc));
                }

                *i_sa = sa(typedef.typedef.size(), typedef.typedef.align());
            }
            _ => {}
        }

        match tc_type.array_kind {
            TCArrayKind::None => {}
            TCArrayKind::Fixed(_) => {
                tc_type.pointer_count += 1;
                tc_type.array_kind = TCArrayKind::None;
            }
        }

        return Ok(tc_type);
    }

    #[inline]
    pub fn check_return_type(
        &self,
        decl_idx: u32,
        ast_type: &ASTType,
        pointer_count: u32,
    ) -> Result<TCType, Error> {
        self.check_type(
            decl_idx,
            ast_type,
            DeclReceiver {
                pointer_count,
                ident: !0,
                array_dims: &[],
                loc: NO_FILE,
            },
            false,
        )
    }

    pub fn to_prim_type(
        &self,
        files: &FileDb,
        tc_type: TCType,
        expr_loc: CodeLoc,
    ) -> Result<TCPrimType, Error> {
        let tc_type = self.resolve_typedef(tc_type, expr_loc)?;

        match self.deref(files, tc_type, expr_loc) {
            Ok(tc_type) => {
                if tc_type == VOID {
                    return Ok(TCPrimType::Pointer { stride_length: 1 });
                } else {
                    return Ok(TCPrimType::Pointer {
                        stride_length: tc_type.size(),
                    });
                }
            }
            Err(_) => {}
        }

        if tc_type.pointer_count != 0 || tc_type.array_kind == TCArrayKind::None {}

        let prim_type = match tc_type.kind {
            TCTypeKind::U8 => TCPrimType::U8,
            TCTypeKind::I8 => TCPrimType::I8,
            TCTypeKind::U32 => TCPrimType::U32,
            TCTypeKind::I32 => TCPrimType::I32,
            TCTypeKind::U64 => TCPrimType::U64,
            TCTypeKind::I64 => TCPrimType::I64,
            _ => {
                let expr_message = format!(
                    "expression here found to be type {}",
                    tc_type.display(files)
                );

                return Err(error!(
                    "type of expression cannot be converted to a primitive type or pointer",
                    expr_loc, expr_message
                ));
            }
        };

        return Ok(prim_type);
    }

    pub fn check_type(
        &self,
        decl_idx: u32,
        ast_type: &ASTType,
        recv: DeclReceiver,
        is_stack_local: bool,
    ) -> Result<TCType, Error> {
        let array_kind = match recv.array_dims.len() {
            0 => TCArrayKind::None,
            1 => {
                if recv.array_dims[0] == TC_UNKNOWN_ARRAY_SIZE {
                    TCArrayKind::Fixed(0)
                } else {
                    TCArrayKind::Fixed(recv.array_dims[0])
                }
            }
            _ => return Err(array_dimensions_too_high(recv.loc)),
        };

        use ASTTypeKind as ATK;
        let kind = match &ast_type.kind {
            ATK::Int => TCTypeKind::I32,
            ATK::Unsigned => TCTypeKind::U32,
            ATK::Char => TCTypeKind::I8,
            ATK::UnsignedChar => TCTypeKind::U8,
            ATK::Long | ATK::LongLong => TCTypeKind::I64,
            ATK::UnsignedLong | ATK::UnsignedLongLong => TCTypeKind::U64,
            ATK::Void => TCTypeKind::Void,
            ATK::Struct(decl) => match decl {
                StructDecl::Named(ident) => TCTypeKind::Struct {
                    ident: *ident,
                    sa: self.check_struct_type(*ident, decl_idx, recv.pointer_count, recv.loc)?,
                },
                StructDecl::NamedDef { members, .. } | StructDecl::Unnamed(members) => {
                    return Err(error!(
                        "struct definition not allowed here",
                        ast_type.loc, "struct definition found here"
                    ))
                }
            },
            &ATK::Ident(ident) => {
                let map_err = || typedef_not_defined(ast_type.loc);
                let typedef = self.typedefs.get(&ident).ok_or_else(map_err)?;

                if typedef.defn_idx > decl_idx {
                    return Err(typedef_defined_later(typedef.loc, ast_type.loc));
                }

                TCTypeKind::Ident {
                    ident,
                    sa: sa(typedef.typedef.size(), typedef.typedef.align()),
                }
            }
        };

        let mut tc_type = TCType::new_array(kind, recv.pointer_count, array_kind);
        if is_stack_local {
            return Ok(tc_type);
        }

        match tc_type.array_kind {
            TCArrayKind::None => {}
            TCArrayKind::Fixed(_) => {
                tc_type.pointer_count += 1;
                tc_type.array_kind = TCArrayKind::None;
            }
        }

        return Ok(tc_type);
    }

    pub fn resolve_typedef(&self, mut expr_type: TCType, loc: CodeLoc) -> Result<TCType, Error> {
        let map_err = || {
            error!(
                "couldn't find typedef (This is an error in TCI)",
                loc, "expression found here"
            )
        };
        while let TCTypeKind::Ident { ident, .. } = expr_type.kind {
            if expr_type.pointer_count != 0 {
                break;
            }

            let defn = self.typedefs.get(&ident).ok_or_else(map_err)?;
            expr_type = defn.typedef;
        }
        return Ok(expr_type);
    }

    // TODO make this work with implicit type conversions
    pub fn implicit_convert<'b>(
        &self,
        buckets: BucketListRef<'b>,
        files: &FileDb,
        assign_type: &TCType,
        assign_loc: CodeLoc,
        assign_loc_is_defn: bool,
        expr: TCExpr<'b>,
    ) -> Result<TCExpr<'b>, Error> {
        if assign_type == &expr.expr_type {
            return Ok(expr);
        }

        let expr_type = self.resolve_typedef(expr.expr_type, expr.loc)?;
        let assign_type = self.resolve_typedef(*assign_type, assign_loc)?;
        if assign_type == expr_type {
            return Ok(expr);
        }

        let from = self.to_prim_type(files, expr_type, expr.loc)?;
        let to = self.to_prim_type(files, assign_type, assign_loc)?;
        return Ok(TCExpr {
            kind: TCExprKind::Conv {
                from,
                to,
                expr: buckets.add(expr),
            },
            expr_type: assign_type,
            loc: expr.loc,
        });
    }

    pub fn cast_convert<'b>(
        &self,
        buckets: BucketListRef<'b>,
        files: &FileDb,
        cast_to: TCType,
        cast_to_loc: CodeLoc,
        mut expr: TCExpr<'b>,
    ) -> Result<TCExpr<'b>, Error> {
        let cast_to = self.resolve_typedef(cast_to, cast_to_loc)?;
        expr.expr_type = self.resolve_typedef(expr.expr_type, expr.loc)?;
        if cast_to == expr.expr_type {
            return Ok(expr);
        }

        let key = (
            self.to_prim_type(files, expr.expr_type, expr.loc)?
                .discriminant(),
            self.to_prim_type(files, cast_to, cast_to_loc)?
                .discriminant(),
        );

        let from = self.to_prim_type(files, expr.expr_type, expr.loc)?;
        let to = self.to_prim_type(files, cast_to, cast_to_loc)?;
        return Ok(TCExpr {
            kind: TCExprKind::Conv {
                from,
                to,
                expr: buckets.add(expr),
            },
            expr_type: cast_to,
            loc: expr.loc,
        });
    }

    pub fn check_struct_type(
        &self,
        struct_ident: u32,
        decl_idx: u32,
        pointer_count: u32,
        loc: CodeLoc,
    ) -> Result<SizeAlign, Error> {
        let no_struct = || error!("referenced struct doesn't exist", loc, "struct used here");

        let struct_type = self.structs.get(&struct_ident).ok_or_else(no_struct)?;
        if struct_type.decl_idx > decl_idx {
            return Err(error!(
                "used type declared later in file",
                struct_type.decl_loc, "type is declared here", loc, "type is used here"
            ));
        }

        if let Some(defn) = &struct_type.defn {
            if defn.meta.defn_idx > decl_idx {
                if pointer_count == 0 {
                    return Err(error!(
                        "used type defined later in file",
                        defn.meta.loc, "type is defined here", loc, "type is used here"
                    ));
                }

                return Ok(TC_UNKNOWN_SA);
            }

            return Ok(defn.meta.sa);
        } else if pointer_count == 0 {
            return Err(error!(
                "referenced incomplete type without pointer indirection",
                struct_type.decl_loc, "incomplete type declared here", loc, "type used here"
            ));
        } else {
            // type incomplete but we have a pointer to it
            return Ok(TC_UNKNOWN_SA);
        }
    }

    pub fn check_struct_member(
        &self,
        struct_type: TCType,
        decl_idx: u32,
        loc: CodeLoc,
        member_ident: u32,
    ) -> Result<TCStructMember, Error> {
        let struct_type = self.resolve_typedef(struct_type, loc)?;
        let struct_info = if let TCTypeKind::Struct { ident, .. } = struct_type.kind {
            self.structs.get(&ident).unwrap()
        } else if let TCTypeKind::AnonStruct { loc: defn_loc, sa } = struct_type.kind {
            self.anon_structs.get(&defn_loc).unwrap()
        } else {
            return Err(member_of_non_struct(loc));
        };

        let defn = if let Some(defn) = &struct_info.defn {
            defn
        } else {
            return Err(error!(
                "tried to dereference undefined struct type",
                loc, "member access here"
            ));
        };

        if defn.meta.defn_idx > decl_idx {
            return Err(error!(
                "struct is defined later in the file for struct pointer dereference (order matters in C)",
                defn.meta.loc,
                "struct defined here",
                loc,
                "struct pointer dereferenced here"
            ));
        }

        #[rustfmt::skip]
        let member = if let Some(member) = defn.members.iter().find(|member| member.ident == member_ident) {
            member
        } else {
            return Err(error!(
                "couldn't find member in struct definition",
                defn.meta.loc, "struct defined here", loc, "member accessed here"
            ));
        };

        let mut member = *member;
        if let TCTypeKind::Struct { ident, sa } = &mut member.decl_type.kind {
            if let Some(struct_defn) = &self.structs[ident].defn {
                if struct_defn.meta.defn_idx < decl_idx {
                    *sa = struct_defn.meta.sa;
                }
            }
        }

        return Ok(member);
    }
}

#[derive(Clone, Copy)]
pub struct CheckEnv<'a, 'b> {
    pub buckets: BucketListRef<'b>,
    pub types: &'a TypeEnv,
    pub func_types: &'a HashMap<u32, TCFuncType>,
    pub files: &'a FileDb,
    pub decl_idx: u32,
}

impl<'a, 'b> CheckEnv<'a, 'b> {
    pub fn new(
        buckets: BucketListRef<'b>,
        types: &'a TypeEnv,
        func_types: &'a HashMap<u32, TCFuncType>,
        files: &'a FileDb,
        decl_idx: u32,
    ) -> Self {
        Self {
            buckets,
            types,
            func_types,
            files,
            decl_idx,
        }
    }

    pub fn to_prim_type(&self, tc_type: TCType, expr_loc: CodeLoc) -> Result<TCPrimType, Error> {
        return self.types.to_prim_type(self.files, tc_type, expr_loc);
    }

    pub fn type_eq(&self, l: TCType, r: TCType) -> bool {
        return self.types.type_eq(l, r);
    }

    pub fn resolve_typedef(&self, expr_type: TCType, loc: CodeLoc) -> Result<TCType, Error> {
        return self.types.resolve_typedef(expr_type, loc);
    }

    #[inline]
    pub fn check_return_type(
        &self,
        ast_type: &ASTType,
        pointer_count: u32,
    ) -> Result<TCType, Error> {
        self.types.check_type(
            self.decl_idx,
            ast_type,
            DeclReceiver {
                pointer_count,
                ident: !0,
                array_dims: &[],
                loc: NO_FILE,
            },
            false,
        )
    }

    pub fn check_decl_type(&self, ast_type: &ASTType, recv: DeclReceiver) -> Result<TCType, Error> {
        self.types.check_type(self.decl_idx, ast_type, recv, true)
    }

    #[inline]
    pub fn check_type(&self, ast_type: &ASTType, recv: DeclReceiver) -> Result<TCType, Error> {
        self.types.check_type(self.decl_idx, ast_type, recv, false)
    }

    #[inline]
    pub fn param_convert(
        &self,
        asgn_type: &TCType,
        asgn_loc: CodeLoc,
        expr: TCExpr<'b>,
    ) -> Result<TCExpr<'b>, Error> {
        self.types
            .implicit_convert(self.buckets, self.files, asgn_type, asgn_loc, true, expr)
    }

    #[inline]
    pub fn return_convert(
        &self,
        asgn_type: &TCType,
        asgn_loc: CodeLoc,
        expr: TCExpr<'b>,
    ) -> Result<TCExpr<'b>, Error> {
        self.types
            .implicit_convert(self.buckets, self.files, asgn_type, asgn_loc, true, expr)
    }

    #[inline]
    pub fn deref(&self, tc_type: TCType, value_loc: CodeLoc) -> Result<TCType, Error> {
        return self.types.deref(self.files, tc_type, value_loc);
    }

    #[inline]
    pub fn decl_assign_convert(
        &self,
        asgn_type: &mut TCType,
        asgn_loc: CodeLoc,
        expr: TCExpr<'b>,
    ) -> Result<TCExpr<'b>, Error> {
        if let TCExprKind::BraceList(list) = expr.kind {
            match &mut asgn_type.array_kind {
                TCArrayKind::None => {
                    return Err(error!(
                        "used an initializer list to initialize something other than an array",
                        expr.loc, "initializer list used here"
                    ));
                }
                TCArrayKind::Fixed(len) => {
                    let element_type = TCType::new(asgn_type.kind, asgn_type.pointer_count);
                    if *len == 0 {
                        *len = list.len() as u32;
                    }

                    if list.len() as u32 != *len {
                        return Err(error!(
                            "array length is not the same as declared array length",
                            asgn_loc,
                            "array length declared here",
                            expr.loc,
                            format!("array has length {}", list.len())
                        ));
                    }

                    let mut array_elements = Vec::new();
                    for expr in list {
                        array_elements.push(self.param_convert(&element_type, asgn_loc, *expr)?);
                    }

                    return Ok(TCExpr {
                        kind: TCExprKind::Array(self.buckets.add_array(array_elements)),
                        expr_type: TCType::new_array(
                            asgn_type.kind,
                            asgn_type.pointer_count,
                            TCArrayKind::Fixed(*len),
                        ),
                        loc: expr.loc,
                    });
                }
            }
        }

        if let TCTypeKind::Uninit { .. } = expr.expr_type.kind {
            if asgn_type.array_kind == TCArrayKind::Fixed(0) {
                return Err(
                    error!(
                    "arrays need to be initialized with an initializer list or declared with an explicit size",
                    asgn_loc, "variable declared here"
                ));
            }

            return Ok(TCExpr {
                kind: TCExprKind::Uninit,
                expr_type: *asgn_type,
                loc: expr.loc,
            });
        }

        self.types
            .implicit_convert(self.buckets, self.files, asgn_type, asgn_loc, false, expr)
    }

    #[inline]
    pub fn assign_convert(
        &self,
        asgn_type: &TCType,
        asgn_loc: CodeLoc,
        expr: TCExpr<'b>,
    ) -> Result<TCExpr<'b>, Error> {
        self.types
            .implicit_convert(self.buckets, self.files, asgn_type, asgn_loc, false, expr)
    }

    #[inline]
    pub fn cast_convert(
        &self,
        cast_to: TCType,
        cast_to_loc: CodeLoc,
        expr: TCExpr<'b>,
    ) -> Result<TCExpr<'b>, Error> {
        self.types
            .cast_convert(self.buckets, self.files, cast_to, cast_to_loc, expr)
    }

    #[inline]
    pub fn check_struct_member(
        &self,
        struct_type: TCType,
        loc: CodeLoc,
        member_ident: u32,
    ) -> Result<TCStructMember, Error> {
        self.types
            .check_struct_member(struct_type, self.decl_idx, loc, member_ident)
    }
}

pub struct TypedFuncs<'a> {
    pub types: TypeEnv,
    pub functions: HashMap<u32, TCFunc<'a>>,
    pub file: u32,
}

#[derive(Debug, Clone, Copy)]
pub enum ITypeKind {
    I32, // int
    U32, // unsigned int
    U64, // unsigned long
    I64, // long
    I8,  // char
    U8,  // unsigned char
    Void,
    Struct(u32),
    AnonStruct(CodeLoc),
    Ident(u32),
}

#[derive(Debug, Clone, Copy)]
pub struct IType {
    pub kind: ITypeKind,
    pub pointer_count: u32,
    pub array_kind: TCArrayKind,
}

impl IType {
    pub fn new(kind: ITypeKind, pointer_count: u32, array_kind: TCArrayKind) -> Self {
        Self {
            kind,
            pointer_count,
            array_kind,
        }
    }

    pub fn into(self) -> TCType {
        let kind = match self.kind {
            ITypeKind::I32 => TCTypeKind::I32,
            ITypeKind::U32 => TCTypeKind::U32,
            ITypeKind::I64 => TCTypeKind::I64,
            ITypeKind::U64 => TCTypeKind::U64,
            ITypeKind::I8 => TCTypeKind::I8,
            ITypeKind::U8 => TCTypeKind::U8,
            ITypeKind::Void => TCTypeKind::Void,
            ITypeKind::Struct(ident) => TCTypeKind::Struct {
                ident,
                sa: TC_UNKNOWN_SA,
            },
            ITypeKind::AnonStruct(loc) => TCTypeKind::AnonStruct {
                loc,
                sa: TC_UNKNOWN_SA,
            },
            ITypeKind::Ident(ident) => TCTypeKind::Ident {
                ident,
                sa: TC_UNKNOWN_SA,
            },
        };

        return TCType::new_array(kind, self.pointer_count, self.array_kind);
    }

    pub fn from_rt(ast_type: &ASTType, pointer_count: u32, loc: CodeLoc) -> Result<Self, Error> {
        let (itype, _) = Self::from_parts(ast_type, pointer_count, &[], loc)?;
        return Ok(itype);
    }

    pub fn from_recv<'a>(
        ast_type: &ASTType<'a>,
        recv: DeclReceiver,
        loc: CodeLoc,
    ) -> Result<(IType, Option<StructDecl<'a>>), Error> {
        return Self::from_parts(ast_type, recv.pointer_count, recv.array_dims, loc);
    }

    pub fn from_parts<'a>(
        ast_type: &ASTType<'a>,
        pointer_count: u32,
        array_dims: &[u32],
        loc: CodeLoc,
    ) -> Result<(IType, Option<StructDecl<'a>>), Error> {
        let array_kind = match array_dims.len() {
            0 => TCArrayKind::None,
            1 => {
                if array_dims[0] == TC_UNKNOWN_ARRAY_SIZE {
                    TCArrayKind::Fixed(TC_UNKNOWN_ARRAY_SIZE)
                } else {
                    TCArrayKind::Fixed(array_dims[0])
                }
            }
            _ => return Err(array_dimensions_too_high(loc)),
        };

        use ASTTypeKind as ATK;
        let mut found_rec = None;
        let kind = match &ast_type.kind {
            ATK::Int => ITypeKind::I32,
            ATK::Unsigned => ITypeKind::U32,
            ATK::Char => ITypeKind::I8,
            ATK::UnsignedChar => ITypeKind::U8,
            ATK::Long | ATK::LongLong => ITypeKind::I64,
            ATK::UnsignedLong | ATK::UnsignedLongLong => ITypeKind::U64,
            ATK::Void => ITypeKind::Void,
            &ATK::Struct(decl) => {
                found_rec = Some(decl);
                match decl {
                    StructDecl::Named(ident) => ITypeKind::Struct(ident),
                    StructDecl::NamedDef { ident, .. } => ITypeKind::Struct(ident),
                    StructDecl::Unnamed(_) => ITypeKind::AnonStruct(ast_type.loc),
                }
            }
            &ATK::Ident(ident) => ITypeKind::Ident(ident),
        };

        return Ok((IType::new(kind, pointer_count, array_kind), found_rec));
    }
}

#[derive(Debug, Clone, Copy)]
pub struct UncheckedStructMember {
    pub ident: u32,
    pub member_type: IType,
    pub loc: CodeLoc,
    pub decl_idx: u32,
}

#[derive(Debug)]
pub struct UncheckedStructDefn {
    pub members: Vec<UncheckedStructMember>,
    pub defn_idx: u32,
    pub loc: CodeLoc,
}

pub struct UncheckedStruct {
    pub decl_idx: u32,
    pub decl_loc: CodeLoc,
    pub defn: Option<UncheckedStructDefn>,
}

#[derive(Debug, Clone, Copy)]
pub struct IFuncParam {
    pub ident: u32,
    pub loc: CodeLoc,
}

#[derive(Debug, Clone)]
pub struct IFuncType {
    pub decl_idx: u32,
    pub return_type: IType,
    pub loc: CodeLoc,
    pub params: Vec<(IType, CodeLoc)>,
    pub varargs: bool,
    pub is_static: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct ITypedef {
    pub def: IType,
    pub defn_idx: u32,
    pub loc: CodeLoc,
}

pub struct UncheckedFunc<'b> {
    pub decl_idx: u32,
    pub decls: Vec<IFuncType>,
    pub defn: Option<UncheckedFuncDefn<'b>>,
}

pub struct UncheckedFuncDefn<'a> {
    pub defn_idx: u32,
    pub loc: CodeLoc,
    pub params: Vec<IFuncParam>,
    pub body: &'a [Stmt<'a>],
}

pub struct UncheckedEnv<'b> {
    pub funcs: HashMap<u32, UncheckedFunc<'b>>,
    pub struct_types: HashMap<u32, UncheckedStruct>,
    pub anon_struct_types: HashMap<CodeLoc, UncheckedStruct>,
    pub typedefs: HashMap<u32, ITypedef>, // TODO what if someone redefines a typedef?
    pub builtins_enabled: Option<u32>,
}

pub fn sequentialize<'a, 'b>(
    buckets: BucketListRef<'a>,
    program: ASTProgram<'b>,
    files: &FileDb,
) -> Result<UncheckedEnv<'b>, Error> {
    let mut env = UncheckedEnv {
        funcs: HashMap::new(),
        struct_types: HashMap::new(),
        anon_struct_types: HashMap::new(),
        typedefs: HashMap::new(),
        builtins_enabled: None,
    };

    let mut decl_idx = 0;

    for stmt in program.stmts {
        sequentialize_rec(buckets, files, &mut decl_idx, &mut env, stmt)?;
    }

    return Ok(env);
}

pub fn sequentialize_struct_defn<'a, 'b>(
    buckets: BucketListRef<'a>,
    files: &FileDb,
    g_decl_idx: &mut u32,
    env: &mut UncheckedEnv<'b>,
    members: &[InnerStructDecl<'b>],
    loc: CodeLoc,
) -> Result<UncheckedStruct, Error> {
    let decl_idx = *g_decl_idx;
    *g_decl_idx += 1;

    let mut unchecked_struct = UncheckedStruct {
        decl_idx,
        decl_loc: loc,
        defn: None,
    };

    let mut names = HashMap::new();
    let mut semi_typed_members = Vec::new();
    for member in members {
        let (member_type, decl_opt) = IType::from_recv(&member.decl_type, member.recv, member.loc)?;

        if let Some(decl) = decl_opt {
            sequentialize_rec(
                buckets,
                files,
                g_decl_idx,
                env,
                &GlobalStmt {
                    kind: GlobalStmtKind::StructDecl(decl),
                    loc: member.decl_type.loc,
                },
            )?;
        }

        let decl_idx = *g_decl_idx;
        *g_decl_idx += 1;

        let tc_member = UncheckedStructMember {
            ident: member.recv.ident,
            decl_idx,
            member_type,
            loc: member.loc,
        };

        semi_typed_members.push(tc_member);
        if let Some(original_loc) = names.insert(member.recv.ident, member.loc) {
            return Err(error!(
                "name redefined in struct",
                original_loc, "first use of name here", member.loc, "second use here"
            ));
        }
    }

    let defn_idx = *g_decl_idx;
    *g_decl_idx += 1;

    let struct_defn = UncheckedStructDefn {
        defn_idx,
        loc,
        members: semi_typed_members,
    };
    unchecked_struct.defn = Some(struct_defn);
    return Ok(unchecked_struct);
}

pub fn sequentialize_struct_decl<'a, 'b>(
    buckets: BucketListRef<'a>,
    files: &FileDb,
    g_decl_idx: &mut u32,
    env: &mut UncheckedEnv<'b>,
    struct_decl: StructDecl<'b>,
    loc: CodeLoc,
) -> Result<(), Error> {
    let (ident, members) = match struct_decl {
        StructDecl::Named(ident) => (ident, None),
        StructDecl::NamedDef { ident, members } => (ident, Some(members)),
        StructDecl::Unnamed(members) => {
            let unchecked_struct =
                sequentialize_struct_defn(buckets, files, g_decl_idx, env, members, loc)?;

            env.anon_struct_types.insert(loc, unchecked_struct);
            return Ok(());
        }
    };

    let members = if let Some(members) = members {
        members
    } else {
        if !env.struct_types.contains_key(&ident) {
            let decl_idx = *g_decl_idx;
            *g_decl_idx += 1;

            env.struct_types.insert(
                ident,
                UncheckedStruct {
                    decl_loc: loc,
                    decl_idx,
                    defn: None,
                },
            );
        }

        return Ok(());
    };

    let mut original_decl_meta = None;

    if let Some(original) = env.struct_types.get(&ident) {
        if let Some(_) = original.defn {
            return Err(error!(
                "redefinition of struct",
                original.decl_loc, "original definition here", loc, "second definition here"
            ));
        }

        original_decl_meta = Some((original.decl_idx, original.decl_loc));
    }

    let mut unchecked_struct =
        sequentialize_struct_defn(buckets, files, g_decl_idx, env, members, loc)?;

    if let Some((decl_idx, decl_loc)) = original_decl_meta {
        unchecked_struct.decl_idx = decl_idx;
        unchecked_struct.decl_loc = decl_loc;
    }

    env.struct_types.insert(ident, unchecked_struct);
    return Ok(());
}

pub fn sequentialize_rec<'a, 'b>(
    buckets: BucketListRef<'a>,
    files: &FileDb,
    g_decl_idx: &mut u32,
    env: &mut UncheckedEnv<'b>,
    global_stmt: &GlobalStmt<'b>,
) -> Result<(), Error> {
    let (rtype, rpointer_count, ident, func_params, func_body) = match global_stmt.kind {
        GlobalStmtKind::FuncDecl {
            return_type,
            ident,
            pointer_count,
            params,
        } => (return_type, pointer_count, ident, params, None),
        GlobalStmtKind::Func {
            return_type,
            ident,
            pointer_count,
            params,
            body,
        } => (return_type, pointer_count, ident, params, Some(body)),
        GlobalStmtKind::StructDecl(decl_type) => {
            sequentialize_struct_decl(buckets, files, g_decl_idx, env, decl_type, global_stmt.loc)?;
            return Ok(());
        }
        GlobalStmtKind::Decl { decl_type, decls } => {
            if let ASTTypeKind::Struct(decl) = decl_type.kind {
                sequentialize_struct_decl(buckets, files, g_decl_idx, env, decl, decl_type.loc)?;
            }

            unimplemented!();
        }
        GlobalStmtKind::Typedef { ast_type, recv } => {
            let (def, decl) = IType::from_recv(&ast_type, recv, global_stmt.loc)?;
            if let Some(decl) = decl {
                sequentialize_struct_decl(buckets, files, g_decl_idx, env, decl, ast_type.loc)?;
            }

            // TODO this ignores redefinition of typedefs
            if env.typedefs.contains_key(&recv.ident) {
                return Ok(());
            }

            let defn_idx = *g_decl_idx;
            *g_decl_idx += 1;

            env.typedefs.insert(
                recv.ident,
                ITypedef {
                    def,
                    defn_idx,
                    loc: global_stmt.loc,
                },
            );

            return Ok(());
        }
        GlobalStmtKind::PragmaEnableBuiltins => {
            let defn_idx = *g_decl_idx;
            *g_decl_idx += 1;

            env.builtins_enabled = Some(env.builtins_enabled.unwrap_or(defn_idx));
            return Ok(());
        }
    };

    let decl_idx = *g_decl_idx;
    *g_decl_idx += 1;

    let return_type = IType::from_rt(&rtype, rpointer_count, rtype.loc)?;

    let mut names = HashMap::new();
    let mut param_types = Vec::new();
    let mut params = Vec::new();
    let mut missing_ident_loc = None;
    let mut varargs = None;
    for param in func_params.iter() {
        if let Some(loc) = varargs {
            return Err(error!(
                "function parameter after vararg",
                loc, "vararg indicator here", param.loc, "parameter here"
            ));
        }

        let (decl_type, pointer_count, array_dims) = match &param.kind {
            ParamKind::Vararg => {
                varargs = Some(param.loc);
                continue;
            }
            ParamKind::StructLike { decl_type, recv } => {
                if missing_ident_loc.is_none() {
                    if let Some(original) = names.insert(recv.ident, param.loc) {
                        return Err(error!(
                            "redeclaration of function parameter",
                            original,
                            "original declaration here",
                            param.loc,
                            "second declaration here"
                        ));
                    }

                    params.push(IFuncParam {
                        ident: recv.ident,
                        loc: param.loc,
                    });
                }

                (decl_type, recv.pointer_count, recv.array_dims)
            }
            ParamKind::TypeOnly {
                decl_type,
                pointer_count,
                array_dims,
            } => {
                if missing_ident_loc.is_none() {
                    missing_ident_loc = Some(param.loc);
                }

                (decl_type, *pointer_count, *array_dims)
            }
        };

        let (param_type, _) = IType::from_parts(&decl_type, pointer_count, array_dims, param.loc)?;
        param_types.push((param_type, param.loc));
    }

    let func_type = IFuncType {
        return_type,
        loc: global_stmt.loc,
        params: param_types,
        decl_idx,
        varargs: varargs.is_some(),
        is_static: rtype.is_static,
    };

    let defn = if let Some(body) = func_body {
        if let Some(ident_loc) = missing_ident_loc {
            return Err(error!(
                "need to give names for all parameters when defining a function",
                ident_loc, "parameter without name found here"
            ));
        }

        let defn_idx = *g_decl_idx;
        *g_decl_idx += 1;

        Some(UncheckedFuncDefn {
            defn_idx,
            params,
            loc: global_stmt.loc,
            body,
        })
    } else {
        None
    };

    if let Some(prev_func) = env.funcs.get_mut(&ident) {
        if let Some(body) = &prev_func.defn {
            if let Some(body_2) = defn {
                return Err(func_redef(body.loc, body_2.loc));
            }
        }

        prev_func.decls.push(func_type);
        prev_func.defn = prev_func.defn.take().or(defn);
    } else {
        env.funcs.insert(
            ident,
            UncheckedFunc {
                decl_idx: func_type.decl_idx,
                decls: vec![func_type],
                defn,
            },
        );
    }
    return Ok(());
}

pub struct Visited {
    pub structs: HashSet<u32>,
    pub anon_structs: HashSet<CodeLoc>,
    pub typedefs: HashSet<u32>,
}

fn check_typedef(
    types: &mut TypeEnv,
    visited: &mut Visited,
    unchecked: &UncheckedEnv,
    current_ident: u32,
    loc: CodeLoc,
) -> Result<(TCStructDefnMeta, TCType), Error> {
    let map_err = || error!("typedef does not exist", loc, "typedef referenced here");
    let typedef = unchecked.typedefs.get(&current_ident).ok_or_else(map_err)?;

    if let ITypeKind::Ident(id) = typedef.def.kind {
        let other = unchecked.typedefs.get(&id).ok_or_else(map_err)?;
        if other.defn_idx > typedef.defn_idx {
            return Err(typedef_defined_later(other.loc, typedef.loc));
        }
    }

    let tc_type = match typedef.def.kind {
        ITypeKind::Struct(ident) => {
            let meta = check_named_struct_type(types, visited, unchecked, ident, typedef.loc)?;
            let meta = meta.ok_or_else(|| member_incomplete_type(typedef.loc))?;

            if meta.defn_idx > typedef.defn_idx {
                return Err(struct_defined_later(meta.loc, typedef.loc));
            }

            let mut tc_type = typedef.def.into();
            tc_type.kind = TCTypeKind::Struct { ident, sa: meta.sa };
            tc_type
        }
        ITypeKind::AnonStruct(loc) => {
            let meta = check_unnamed_struct_type(types, visited, unchecked, loc, typedef.loc)?;

            if meta.defn_idx > typedef.defn_idx {
                return Err(struct_defined_later(meta.loc, typedef.loc));
            }

            let mut tc_type = typedef.def.into();
            tc_type.kind = TCTypeKind::AnonStruct { loc, sa: meta.sa };
            tc_type
        }
        ITypeKind::Ident(ident) => {
            let (meta, tc_type) = check_typedef(types, visited, unchecked, ident, typedef.loc)?;

            if meta.defn_idx > typedef.defn_idx {
                return Err(typedef_defined_later(meta.loc, typedef.loc));
            }

            tc_type
        }
        _ => typedef.def.into(),
    };

    let typedef_meta = TCStructDefnMeta {
        defn_idx: typedef.defn_idx,
        loc: typedef.loc,
        sa: sa(tc_type.size(), tc_type.align()),
    };

    types.typedefs.insert(
        current_ident,
        TCTypedef {
            typedef: tc_type,
            defn_idx: typedef_meta.defn_idx,
            loc,
        },
    );

    return Ok((typedef_meta, tc_type));
}

fn check_unnamed_struct_type(
    types: &mut TypeEnv,
    visited: &mut Visited,
    unchecked: &UncheckedEnv,
    defn_loc: CodeLoc,
    loc: CodeLoc,
) -> Result<TCStructDefnMeta, Error> {
    if visited.anon_structs.contains(&defn_loc) {
        let found = types.anon_structs.get(&defn_loc).unwrap();
        return Ok(found.defn.as_ref().unwrap().meta);
    }

    let type_decl = if let Some(type_decl) = unchecked.anon_struct_types.get(&defn_loc) {
        type_decl
    } else {
        return Err(error!(
            "anonymous struct does not exist (this is an error in TCI)",
            defn_loc,
            format!("we thought it would be defined here ({:?})", defn_loc)
        ));
    };

    let checked_defn =
        check_struct_type(types, visited, unchecked, type_decl.defn.as_ref().unwrap())?;
    let meta = checked_defn.meta;

    visited.anon_structs.insert(defn_loc);

    types.anon_structs.insert(
        defn_loc,
        TCStruct {
            decl_idx: type_decl.decl_idx,
            decl_loc: type_decl.decl_loc,
            defn: Some(checked_defn),
        },
    );
    return Ok(meta);
}

fn check_named_struct_type(
    types: &mut TypeEnv,
    visited: &mut Visited,
    unchecked: &UncheckedEnv,
    current_ident: u32,
    loc: CodeLoc,
) -> Result<Option<TCStructDefnMeta>, Error> {
    let type_decl = if let Some(type_decl) = unchecked.struct_types.get(&current_ident) {
        type_decl
    } else {
        return Err(error!( // TODO will this ever trigger? I don't think so
            "struct does not exist",
            loc, "struct referenced here"
        ));
    };

    if visited.structs.contains(&current_ident) {
        if let Some(found) = types.structs.get(&current_ident) {
            if let Some(defn) = &found.defn {
                return Ok(Some(defn.meta));
            }
            return Ok(None);
        } else {
            return Err(error!(
                "struct heirarchy contains cycle",
                type_decl.decl_loc, "found cycle while solving this type"
            ));
        }
    }

    visited.structs.insert(current_ident);

    let defn = if let Some(defn) = &type_decl.defn {
        defn
    } else {
        types.structs.insert(
            current_ident,
            TCStruct {
                decl_idx: type_decl.decl_idx,
                decl_loc: type_decl.decl_loc,
                defn: None,
            },
        );

        return Ok(None);
    };

    let checked_defn = check_struct_type(types, visited, unchecked, defn)?;
    let meta = checked_defn.meta;

    types.structs.insert(
        current_ident,
        TCStruct {
            decl_idx: type_decl.decl_idx,
            decl_loc: type_decl.decl_loc,
            defn: Some(checked_defn),
        },
    );

    return Ok(Some(meta));
}

fn check_struct_type<'b>(
    types: &mut TypeEnv,
    visited: &mut Visited,
    unchecked: &UncheckedEnv,
    defn: &UncheckedStructDefn,
) -> Result<TCStructDefn, Error> {
    let mut size: u32 = 0;
    let mut align: u32 = 0;
    let mut typed_members = Vec::new();

    for member in defn.members.iter() {
        if member.member_type.pointer_count != 0 {
            let offset = align_u32(size, 8);
            size = offset + 8;
            align = u32::max(8, align);

            typed_members.push(TCStructMember {
                ident: member.ident,
                decl_type: member.member_type.into(),
                loc: member.loc,
                offset,
                decl_idx: member.decl_idx,
            });

            continue;
        }

        let tc_type = match member.member_type.kind {
            ITypeKind::Struct(ident) => {
                let meta = check_named_struct_type(types, visited, unchecked, ident, member.loc)?;
                let meta = meta.ok_or_else(|| member_incomplete_type(member.loc))?;

                if meta.defn_idx > member.decl_idx {
                    return Err(struct_defined_later(meta.loc, member.loc));
                }

                let mut tc_type = member.member_type.into();
                tc_type.kind = TCTypeKind::Struct { ident, sa: meta.sa };
                tc_type
            }
            ITypeKind::AnonStruct(loc) => {
                let meta = check_unnamed_struct_type(types, visited, unchecked, loc, member.loc)?;

                if meta.defn_idx > member.decl_idx {
                    // TODO can this ever go off?
                    return Err(struct_defined_later(meta.loc, member.loc));
                }

                let mut tc_type = member.member_type.into();
                tc_type.kind = TCTypeKind::AnonStruct { loc, sa: meta.sa };
                tc_type
            }
            ITypeKind::Ident(ident) => {
                let (meta, tc_type) = check_typedef(types, visited, unchecked, ident, member.loc)?;

                if meta.defn_idx > member.decl_idx {
                    return Err(typedef_defined_later(meta.loc, member.loc));
                }

                tc_type
            }
            _ => member.member_type.into(),
        };

        // m prefix to mean member's size align (m_size)
        let (m_size, m_align) = (tc_type.size(), tc_type.align());
        let offset = align_u32(size, m_align);
        size = offset + m_size;
        align = u32::max(m_align, align);

        typed_members.push(TCStructMember {
            ident: member.ident,
            decl_type: tc_type,
            loc: member.loc,
            offset,
            decl_idx: member.decl_idx,
        });
    }

    let sa = sa(align_u32(size, align), align);
    let meta = TCStructDefnMeta {
        defn_idx: defn.defn_idx,
        loc: defn.loc,
        sa,
    };

    let checked_defn = TCStructDefn {
        members: typed_members,
        meta,
    };

    return Ok(checked_defn);
}

pub fn check_file<'a>(
    buckets: BucketListRef<'a>,
    program: ASTProgram,
    file: u32,
    files: &FileDb,
) -> Result<TypedFuncs<'a>, Error> {
    let unchecked_env = sequentialize(buckets, program, files)?;
    let mut types = TypeEnv::new(unchecked_env.builtins_enabled);

    let mut visited = Visited {
        structs: HashSet::new(),
        anon_structs: HashSet::new(),
        typedefs: HashSet::new(),
    };

    for (ident, unchecked) in unchecked_env.struct_types.iter() {
        check_named_struct_type(
            &mut types,
            &mut visited,
            &unchecked_env,
            *ident,
            unchecked.decl_loc,
        )?;
    }

    for (defn_loc, unchecked) in unchecked_env.anon_struct_types.iter() {
        check_unnamed_struct_type(
            &mut types,
            &mut visited,
            &unchecked_env,
            *defn_loc,
            unchecked.decl_loc,
        )?;
    }

    for (ident, unchecked) in unchecked_env.typedefs.iter() {
        check_typedef(
            &mut types,
            &mut visited,
            &unchecked_env,
            *ident,
            unchecked.loc,
        )?;
    }

    // TODO what if someone redefines a typedef?

    let mut func_types = HashMap::new();
    for (func_name, func) in unchecked_env.funcs.iter() {
        let decl_idx = func.decls[0].decl_idx;

        let check_func_type = |func_type: &IFuncType| -> Result<TCFuncType, Error> {
            let return_type =
                types.check_func_itype(decl_idx, func_type.return_type, func_type.loc)?;

            let mut params = Vec::new();
            for (param_type, param_loc) in func_type.params.iter() {
                let decl_type = types.check_func_itype(decl_idx, *param_type, *param_loc)?;
                params.push((decl_type, *param_loc));
            }

            return Ok(TCFuncType {
                decl_idx,
                return_type,
                params,
                loc: func_type.loc,
                varargs: func_type.varargs,
                is_static: func_type.is_static,
            });
        };

        let func_type = check_func_type(&func.decls[0])?;
        for ftype in func.decls.iter().skip(1) {
            let second_func_type = check_func_type(ftype)?;
            if func_type != second_func_type {
                return Err(func_decl_mismatch(func_type.loc, second_func_type.loc));
            }
        }

        func_types.insert(*func_name, func_type);
    }

    if let Some(func_type) = func_types.get(&INIT_SYMS.translate["main"]) {
        if func_type.return_type.pointer_count != 0 {
            return Err(main_return_type(func_type.loc));
        }

        match func_type.return_type.kind {
            TCTypeKind::Void | TCTypeKind::I32 => {}
            _ => {
                return Err(main_return_type(func_type.loc));
            }
        }

        let int_type = TCType::new(TCTypeKind::I32, 0);
        let char_ss_type = TCType::new(TCTypeKind::I8, 2);

        if func_type.params.len() == 2 {
            if func_type.params[0].0 != int_type {
                return Err(main_param_types(func_type.params[0].1));
            } else if func_type.params[1].0 != char_ss_type {
                return Err(main_param_types(func_type.loc));
            }
        } else if func_type.params.len() != 0 {
            return Err(main_param_types(func_type.loc));
        }
    }

    let mut func_defs = HashMap::new();
    for (func_name, func) in unchecked_env.funcs.into_iter() {
        let defn = match func.defn {
            Some(defn) => defn,
            None => continue,
        };
        let func_type = &func_types[&func_name];

        let mut local_env = LocalTypeEnv::new(func_type.return_type, func_type.loc);
        let mut params = Vec::new();

        for (idx, &(param_type, param_type_loc)) in func_type.params.iter().enumerate() {
            let param = defn.params[idx];
            let var_offset = idx as i16 * -1 - 2;
            let tc_value = TCVar {
                decl_type: param_type,
                var_offset,
                loc: param.loc,
            };

            local_env.add_var(param.ident, tc_value).unwrap(); // this is handled in sequentialize
            params.push(TCFuncParam {
                param_type,
                loc: param.loc,
                ident: param.ident,
            });
        }

        let env = CheckEnv::new(buckets, &types, &func_types, files, defn.defn_idx);

        let gstmts = check_stmts(env, &mut local_env, defn.body, None)?;
        func_defs.insert(
            func_name,
            TCFuncDefn {
                defn_idx: defn.defn_idx,
                loc: defn.loc,
                params,
                stmts: env.buckets.add_array(gstmts),
            },
        );
    }

    let mut functions = HashMap::new();
    for (func_name, func_type) in func_types.into_iter() {
        let defn = func_defs.remove(&func_name);
        functions.insert(func_name, TCFunc { func_type, defn });
    }

    return Ok(TypedFuncs {
        types,
        functions,
        file,
    });
}

fn check_stmts<'b>(
    env: CheckEnv<'_, 'b>,
    local_env: &mut LocalTypeEnv,
    stmts: &[Stmt],
    cblock: Option<TCStmt<'b>>,
) -> Result<Vec<TCStmt<'b>>, Error> {
    let mut tstmts = Vec::new();
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::RetVal(expr) => {
                let expr = check_expr(env, local_env, expr)?;
                let rtype = local_env.return_type;
                if rtype.pointer_count == 0
                    && rtype.kind == TCTypeKind::Void
                    && rtype != expr.expr_type
                {
                    return Err(error!(
                        "void function should not return a value",
                        expr.loc, "value is here"
                    ));
                }

                let expr = env.return_convert(&local_env.return_type, local_env.rtype_loc, expr)?;

                tstmts.push(TCStmt {
                    loc: stmt.loc,
                    kind: TCStmtKind::RetVal(expr),
                });
            }
            StmtKind::Ret => {
                let rtype = local_env.return_type;
                if rtype.pointer_count != 0 || rtype.kind != TCTypeKind::Void {
                    return Err(error!(
                        "expected value in return statement (return type is not void)",
                        local_env.rtype_loc,
                        "target type is here".to_string(),
                        stmt.loc,
                        "return statement is here".to_string()
                    ));
                }

                tstmts.push(TCStmt {
                    loc: stmt.loc,
                    kind: TCStmtKind::Ret,
                });
            }

            StmtKind::Expr(expr) => {
                let expr = check_expr(env, local_env, expr)?;
                if expr.expr_type.kind == TCTypeKind::BraceList {
                    return Err(brace_list(expr.loc));
                }

                tstmts.push(TCStmt {
                    loc: expr.loc,
                    kind: TCStmtKind::Expr(expr),
                });
            }

            StmtKind::Decl { decl_type, decls } => {
                for Decl { recv, loc, expr } in *decls {
                    let mut decl_type = env.check_decl_type(decl_type, *recv)?;
                    if decl_type == VOID {
                        return Err(void_variable(*loc));
                    }

                    let expr = check_expr_allow_brace(env, local_env, &expr)?;
                    let expr = env.decl_assign_convert(&mut decl_type, recv.loc, expr)?;
                    local_env.add_local(recv.ident, decl_type, *loc)?;
                    tstmts.push(TCStmt {
                        kind: TCStmtKind::Decl {
                            symbol: recv.ident,
                            init: expr,
                        },
                        loc: *loc,
                    });
                }
            }

            StmtKind::Nop => {}

            StmtKind::Branch {
                if_cond,
                if_body,
                else_body,
            } => {
                let cond = check_expr(env, local_env, if_cond)?;
                if let TCTypeKind::Struct { .. } = cond.expr_type.kind {
                    return Err(truth_value_of_struct(cond.loc));
                }

                let mut if_env = local_env.child();
                let tc_if_body = check_stmts(env, &mut if_env, if_body.stmts, cblock)?;

                let mut else_env = local_env.child();
                let tc_else_body = check_stmts(env, &mut else_env, else_body.stmts, cblock)?;

                let tc_if_body = env.buckets.add_array(tc_if_body);
                let tc_else_body = env.buckets.add_array(tc_else_body);

                tstmts.push(TCStmt {
                    kind: TCStmtKind::Branch {
                        cond,
                        if_body: TCBlock {
                            stmts: tc_if_body,
                            loc: if_body.loc,
                        },
                        else_body: TCBlock {
                            stmts: tc_else_body,
                            loc: else_body.loc,
                        },
                    },
                    loc: stmt.loc,
                });
            }

            StmtKind::For {
                at_start,
                condition,
                post_expr,
                body,
            } => {
                let mut block_stmts = Vec::new();
                let at_start = check_expr(env, local_env, at_start)?;
                block_stmts.push(TCStmt {
                    loc: at_start.loc,
                    kind: TCStmtKind::Expr(at_start),
                });

                let cond = check_expr(env, local_env, condition)?;
                if let TCTypeKind::Struct { .. } = cond.expr_type.kind {
                    return Err(truth_value_of_struct(cond.loc));
                }

                let post = check_expr(env, local_env, post_expr)?;
                let post = TCStmt {
                    loc: post.loc,
                    kind: TCStmtKind::Expr(post),
                };

                let mut for_env = local_env.child();
                let mut loop_stmts = check_stmts(env, &mut for_env, body.stmts, Some(post))?;

                loop_stmts.push(post);

                loop_stmts.push(TCStmt {
                    loc: condition.loc,
                    kind: TCStmtKind::Branch {
                        if_body: TCBlock {
                            stmts: &[],
                            loc: condition.loc,
                        },
                        else_body: TCBlock {
                            stmts: env.buckets.add_array(vec![TCStmt {
                                kind: TCStmtKind::Break,
                                loc: condition.loc,
                            }]),
                            loc: condition.loc,
                        },
                        cond,
                    },
                });

                loop_stmts.rotate_right(1);

                block_stmts.push(TCStmt {
                    loc: body.loc,
                    kind: TCStmtKind::Loop(TCBlock {
                        loc: body.loc,
                        stmts: env.buckets.add_array(loop_stmts),
                    }),
                });

                tstmts.push(TCStmt {
                    kind: TCStmtKind::Block(TCBlock {
                        loc: stmt.loc,
                        stmts: env.buckets.add_array(block_stmts),
                    }),
                    loc: stmt.loc,
                });
            }
            StmtKind::ForDecl {
                at_start_decl_type,
                at_start,
                condition,
                post_expr,
                body,
            } => {
                let mut block_stmts = Vec::new();
                let mut for_env = local_env.child();

                for decl in *at_start {
                    let mut decl_type = env.check_type(at_start_decl_type, decl.recv)?;
                    if decl_type == VOID {
                        return Err(void_variable(decl.loc));
                    }

                    let expr = check_expr_allow_brace(env, &mut for_env, &decl.expr)?;
                    let expr = env.decl_assign_convert(&mut decl_type, decl.loc, expr)?;
                    for_env.add_local(decl.recv.ident, decl_type, decl.loc)?;
                    block_stmts.push(TCStmt {
                        kind: TCStmtKind::Decl {
                            symbol: decl.recv.ident,
                            init: expr,
                        },
                        loc: decl.loc,
                    });
                }

                let cond = check_expr(env, &for_env, condition)?;
                if let TCTypeKind::Struct { .. } = cond.expr_type.kind {
                    return Err(truth_value_of_struct(cond.loc));
                }

                let post = check_expr(env, &for_env, post_expr)?;
                let post = TCStmt {
                    loc: post.loc,
                    kind: TCStmtKind::Expr(post),
                };

                let mut loop_stmts = check_stmts(env, &mut for_env, body.stmts, Some(post))?;

                loop_stmts.push(post);

                loop_stmts.push(TCStmt {
                    loc: condition.loc,
                    kind: TCStmtKind::Branch {
                        if_body: TCBlock {
                            stmts: &[],
                            loc: condition.loc,
                        },
                        else_body: TCBlock {
                            stmts: env.buckets.add_array(vec![TCStmt {
                                kind: TCStmtKind::Break,
                                loc: condition.loc,
                            }]),
                            loc: condition.loc,
                        },
                        cond,
                    },
                });

                loop_stmts.rotate_right(1);

                block_stmts.push(TCStmt {
                    loc: body.loc,
                    kind: TCStmtKind::Loop(TCBlock {
                        loc: body.loc,
                        stmts: env.buckets.add_array(loop_stmts),
                    }),
                });

                tstmts.push(TCStmt {
                    kind: TCStmtKind::Block(TCBlock {
                        loc: stmt.loc,
                        stmts: env.buckets.add_array(block_stmts),
                    }),
                    loc: stmt.loc,
                });
            }

            StmtKind::While { condition, body } => {
                let cond = check_expr(env, local_env, condition)?;
                if let TCTypeKind::Struct { .. } = cond.expr_type.kind {
                    return Err(truth_value_of_struct(cond.loc));
                }

                let mut while_env = local_env.child();
                let mut loop_stmts = check_stmts(env, &mut while_env, body.stmts, None)?;

                loop_stmts.push(TCStmt {
                    loc: condition.loc,
                    kind: TCStmtKind::Branch {
                        if_body: TCBlock {
                            stmts: &[],
                            loc: condition.loc,
                        },
                        else_body: TCBlock {
                            stmts: env.buckets.add_array(vec![TCStmt {
                                kind: TCStmtKind::Break,
                                loc: condition.loc,
                            }]),
                            loc: condition.loc,
                        },
                        cond,
                    },
                });

                loop_stmts.rotate_right(1);

                tstmts.push(TCStmt {
                    kind: TCStmtKind::Loop(TCBlock {
                        loc: body.loc,
                        stmts: env.buckets.add_array(loop_stmts),
                    }),
                    loc: stmt.loc,
                });
            }

            StmtKind::Block(block) => {
                let mut block_env = local_env.child();
                let block_stmts = check_stmts(env, &mut block_env, block.stmts, cblock)?;
                tstmts.push(TCStmt {
                    kind: TCStmtKind::Block(TCBlock {
                        loc: stmt.loc,
                        stmts: env.buckets.add_array(block_stmts),
                    }),
                    loc: stmt.loc,
                });
            } // x => panic!("{:?} is unimplemented", x),

            StmtKind::Continue => {
                if let Some(cblock) = cblock {
                    tstmts.push(cblock);
                }

                tstmts.push(TCStmt {
                    kind: TCStmtKind::Continue,
                    loc: stmt.loc,
                });
            }
            StmtKind::Break => {
                tstmts.push(TCStmt {
                    kind: TCStmtKind::Break,
                    loc: stmt.loc,
                });
            }
        }
    }

    return Ok(tstmts);
}

pub fn check_expr<'b>(
    env: CheckEnv<'_, 'b>,
    local_env: &LocalTypeEnv,
    expr: &Expr,
) -> Result<TCExpr<'b>, Error> {
    let expr = check_expr_allow_brace(env, local_env, expr)?;
    if expr.expr_type.kind == TCTypeKind::BraceList {
        return Err(brace_list(expr.loc));
    }

    return Ok(expr);
}

pub fn check_expr_allow_brace<'b>(
    env: CheckEnv<'_, 'b>,
    local_env: &LocalTypeEnv,
    expr: &Expr,
) -> Result<TCExpr<'b>, Error> {
    match expr.kind {
        ExprKind::Uninit => {
            return Ok(TCExpr {
                kind: TCExprKind::Uninit,
                expr_type: TCType::new(TCTypeKind::Uninit { size: 0 }, 0),
                loc: expr.loc,
            });
        }
        ExprKind::IntLiteral(val) => {
            return Ok(TCExpr {
                kind: TCExprKind::I32Literal(val),
                expr_type: TCType::new(TCTypeKind::I32, 0),
                loc: expr.loc,
            });
        }
        ExprKind::StringLiteral(val) => {
            return Ok(TCExpr {
                kind: TCExprKind::StringLiteral(env.buckets.add_str(val)),
                expr_type: TCType::new(TCTypeKind::I8, 1),
                loc: expr.loc,
            });
        }
        ExprKind::CharLiteral(c) => {
            return Ok(TCExpr {
                kind: TCExprKind::I8Literal(c),
                expr_type: TCType::new(TCTypeKind::I8, 0),
                loc: expr.loc,
            });
        }
        ExprKind::Ident(id) => {
            let tc_var = match local_env.var(id) {
                Some(tc_var) => tc_var,
                None => {
                    return Err(error!("couldn't find name", expr.loc, "identifier here"));
                }
            };

            return Ok(TCExpr {
                kind: TCExprKind::LocalIdent {
                    var_offset: tc_var.var_offset,
                },
                expr_type: tc_var.decl_type,
                loc: expr.loc,
            });
        }

        ExprKind::SizeofType {
            sizeof_type,
            pointer_count,
        } => {
            let tc_sizeof_type = env.check_return_type(&sizeof_type, pointer_count)?;
            if tc_sizeof_type == VOID {
                return Err(error!(
                    "sizeof called on void type (this doesn't make sense because void doesn't have a size)",
                    expr.loc, "called here"
                ));
            }

            return Ok(TCExpr {
                kind: TCExprKind::U64Literal(tc_sizeof_type.size() as u64),
                expr_type: TCType::new(TCTypeKind::U64, 0),
                loc: expr.loc,
            });
        }
        ExprKind::SizeofExpr(sizeof_expr) => {
            let tc_expr = check_expr(env, local_env, sizeof_expr)?;

            return Ok(TCExpr {
                kind: TCExprKind::U64Literal(tc_expr.expr_type.size() as u64),
                expr_type: TCType::new(TCTypeKind::U64, 0),
                loc: expr.loc,
            });
        }

        ExprKind::BraceList(exprs) => {
            let mut tc_exprs = Vec::new();
            for expr in exprs {
                tc_exprs.push(check_expr_allow_brace(env, local_env, expr)?);
            }

            return Ok(TCExpr {
                expr_type: BRACE_LIST,
                kind: TCExprKind::BraceList(env.buckets.add_array(tc_exprs)),
                loc: expr.loc,
            });
        }
        ExprKind::ParenList(exprs) => {
            let mut tc_exprs = Vec::new();
            for expr in exprs {
                tc_exprs.push(check_expr(env, local_env, expr)?);
            }

            if tc_exprs.len() == 0 {
                return Ok(TCExpr {
                    expr_type: TCType::new(TCTypeKind::Uninit { size: 0 }, 0),
                    kind: TCExprKind::ParenList(&[]),
                    loc: expr.loc,
                });
            }

            return Ok(TCExpr {
                expr_type: tc_exprs[tc_exprs.len() - 1].expr_type,
                kind: TCExprKind::ParenList(env.buckets.add_array(tc_exprs)),
                loc: expr.loc,
            });
        }

        ExprKind::PostIncr(target) => {
            let mut target = check_assign_target(env, local_env, target)?;
            target.target_type = env.resolve_typedef(target.target_type, target.target_loc)?;
            match env.to_prim_type(target.target_type, target.target_loc)? {
                TCPrimType::U64 | TCPrimType::I64 => {
                    return Ok(TCExpr {
                        expr_type: target.target_type,
                        loc: expr.loc,
                        kind: TCExprKind::PostIncrU64(target),
                    });
                }
                TCPrimType::U32 | TCPrimType::I32 => {
                    return Ok(TCExpr {
                        expr_type: target.target_type,
                        loc: expr.loc,
                        kind: TCExprKind::PostIncrU32(target),
                    });
                }
                _ => unimplemented!(),
            }
        }

        ExprKind::Assign(target, value) => {
            let target = check_assign_target(env, local_env, target)?;
            let value = check_expr(env, local_env, value)?;

            let value = env.assign_convert(&target.target_type, target.target_loc, value)?;

            let value = env.buckets.add(value);

            return Ok(TCExpr {
                expr_type: target.target_type,
                loc: expr.loc,
                kind: TCExprKind::Assign { target, value },
            });
        }

        ExprKind::MutAssign { target, value, op } => {
            let target = check_assign_target(env, local_env, target)?;
            let value = check_expr(env, local_env, value)?;

            let target_type = match op {
                BinOp::LShift | BinOp::RShift => TCType::new(TCTypeKind::U8, 0),
                _ => target.target_type,
            };

            let op_type = env.to_prim_type(target.target_type, target.target_loc)?;
            let value = env.assign_convert(&target_type, target.target_loc, value)?;
            let value = env.buckets.add(value);

            return Ok(TCExpr {
                expr_type: target.target_type,
                loc: expr.loc,
                kind: TCExprKind::MutAssign {
                    target,
                    value,
                    op,
                    op_type,
                },
            });
        }

        ExprKind::Ternary {
            condition,
            if_true,
            if_false,
        } => {
            let condition = check_expr(env, local_env, condition)?;
            if let TCTypeKind::Struct { .. } = condition.expr_type.kind {
                return Err(truth_value_of_struct(condition.loc));
            }

            let if_true = check_expr(env, local_env, if_true)?;
            let if_false = check_expr(env, local_env, if_false)?;
            let (if_true, if_false, _) = unify(env, if_true, if_false)?;

            let condition = env.buckets.add(condition);
            let if_true = env.buckets.add(if_true);
            let if_false = env.buckets.add(if_false);

            return Ok(TCExpr {
                expr_type: if_true.expr_type,
                kind: TCExprKind::Ternary {
                    condition,
                    if_true,
                    if_false,
                },
                loc: expr.loc,
            });
        }

        ExprKind::BinOp(op, l, r) => {
            let l = check_expr(env, local_env, l)?;
            let r = check_expr(env, local_env, r)?;

            if let Some(transform) = get_overload(env, op, &l, &r)? {
                return transform(env, l, r);
            }

            let (left, right, op_type) = unify(env, l, r)?;
            let expr_type = match op {
                BinOp::Lt | BinOp::Gt | BinOp::Leq | BinOp::Geq => TCType::new(TCTypeKind::I8, 0),
                BinOp::Eq | BinOp::Neq => TCType::new(TCTypeKind::I8, 0),
                _ => left.expr_type,
            };

            return Ok(TCExpr {
                kind: TCExprKind::BinOp {
                    op,
                    op_type,
                    left: env.buckets.add(left),
                    right: env.buckets.add(right),
                },
                loc: l_from(left.loc, right.loc),
                expr_type,
            });
        }

        ExprKind::UnaryOp(op, operand) => {
            let operand = check_expr(env, local_env, operand)?;
            let prim_operand = env.to_prim_type(operand.expr_type, operand.loc)?;

            let key = (op, prim_operand.discriminant());
            let un_op = match OVERLOADS.unary_op.get(&key) {
                Some(un_op) => *un_op,
                None => {
                    return Err(error!(
                        "invalid operation to unary operand",
                        operand.loc,
                        format!(
                            "operand found here with type {}",
                            operand.expr_type.display(env.files)
                        )
                    ))
                }
            };

            return Ok(un_op(env.buckets, operand, expr.loc));
        }

        ExprKind::Member { base, member } => {
            let base = check_expr(env, local_env, base)?;
            let member_info = env.check_struct_member(base.expr_type, base.loc, member)?;

            return Ok(TCExpr {
                expr_type: member_info.decl_type,
                loc: expr.loc,
                kind: TCExprKind::Member {
                    base: env.buckets.add(base),
                    offset: member_info.offset,
                },
            });
        }
        ExprKind::PtrMember { base, member } => {
            let base = check_expr(env, local_env, base)?;

            let deref_type = env.deref(base.expr_type, base.loc)?;
            if deref_type.pointer_count != 0 {
                return Err(ptr_member_of_poly_pointer(base.loc, &deref_type));
            }

            let member_info = env.check_struct_member(deref_type, base.loc, member)?;

            return Ok(TCExpr {
                expr_type: member_info.decl_type,
                loc: expr.loc,
                kind: TCExprKind::PtrMember {
                    base: env.buckets.add(base),
                    offset: member_info.offset,
                },
            });
        }

        ExprKind::Deref(ptr) => {
            let value = check_expr(env, local_env, ptr)?;

            let expr_type = env.deref(value.expr_type, value.loc)?;
            return Ok(TCExpr {
                expr_type,
                loc: expr.loc,
                kind: TCExprKind::Deref(env.buckets.add(value)),
            });
        }
        ExprKind::Ref(target) => {
            let target = check_assign_target(env, local_env, target)?;
            let mut expr_type = target.target_type;
            expr_type.pointer_count += 1;
            return Ok(TCExpr {
                expr_type,
                loc: expr.loc,
                kind: TCExprKind::Ref(target),
            });
        }

        ExprKind::Call { function, params } => {
            let func_id = if let ExprKind::Ident(id) = function.kind {
                id
            } else {
                return Err(error!(
                    "calling an expression that isn't a function",
                    function.loc, "called here"
                ));
            };

            let func_type = if let Some(func_type) = env.func_types.get(&func_id) {
                func_type
            } else {
                if env.types.builtins_enabled.map(|idx| idx < env.decl_idx) == Some(true) {
                    if let Some(trans) = BUILTINS.get(env.files.symbol_to_str(func_id)) {
                        let (builtin, expr_type) = trans(env, local_env, expr.loc, params)?;
                        return Ok(TCExpr {
                            kind: TCExprKind::Builtin(builtin),
                            loc: expr.loc,
                            expr_type,
                        });
                    }
                }

                return Err(error!("function doesn't exist", expr.loc, "called here"));
            };

            if func_type.decl_idx > env.decl_idx {
                return Err(error!(
                    "function hasn't been declared yet (declaration order matters in C)",
                    expr.loc, "function called here", func_type.loc, "function declared here"
                ));
            }

            if params.len() < func_type.params.len()
                || (params.len() > func_type.params.len() && !func_type.varargs)
            {
                return Err(error!(
                    "function call has wrong number of parameters",
                    expr.loc, "function called here", func_type.loc, "function declared here"
                ));
            }

            let mut tparams = Vec::new();
            for (idx, param) in params.iter().enumerate() {
                let mut expr = check_expr(env, local_env, param)?;
                if idx < func_type.params.len() {
                    let param_type = &func_type.params[idx];
                    expr = env.param_convert(&param_type.0, param_type.1, expr)?;
                }

                tparams.push(expr);
            }

            return Ok(TCExpr {
                kind: TCExprKind::Call {
                    func: func_id,
                    params: env.buckets.add_array(tparams),
                    named_count: params.len() as u32,
                },
                expr_type: func_type.return_type,
                loc: expr.loc,
            });
        }

        ExprKind::Cast {
            cast_to,
            cast_to_loc,
            pointer_count,
            expr,
        } => {
            let cast_to = env.check_return_type(&cast_to, pointer_count)?;
            let expr = check_expr(env, local_env, expr)?;
            return env.cast_convert(cast_to, cast_to_loc, expr);
        }

        x => panic!("{:?} is unimplemented", x),
    }
}

fn check_assign_target<'b>(
    env: CheckEnv<'_, 'b>,
    local_env: &LocalTypeEnv,
    expr: &Expr,
) -> Result<TCAssignTarget<'b>, Error> {
    match &expr.kind {
        ExprKind::Ident(id) => {
            let tc_var = match local_env.var(*id) {
                Some(tc_var) => tc_var,
                None => {
                    return Err(ident_not_found(&env.types, expr.loc));
                }
            };

            let kind = TCAssignTargetKind::LocalIdent {
                var_offset: tc_var.var_offset,
            };

            return Ok(TCAssignTarget {
                kind,
                defn_loc: Some(tc_var.loc),
                target_loc: expr.loc,
                target_type: tc_var.decl_type,
                offset: 0,
            });
        }

        ExprKind::Member { base, member } => {
            let base_loc = base.loc;
            let base = check_assign_target(env, local_env, base)?;

            let member_info =
                env.check_struct_member(base.target_type, base.target_loc, *member)?;

            return Ok(TCAssignTarget {
                kind: base.kind,
                defn_loc: Some(member_info.loc),
                target_loc: expr.loc,
                target_type: member_info.decl_type,
                offset: member_info.offset,
            });
        }
        ExprKind::PtrMember { base, member } => {
            let base_loc = base.loc;
            let base = check_expr(env, local_env, base)?;

            let deref_type = env.deref(base.expr_type, base.loc)?;
            if deref_type.pointer_count != 0 {
                return Err(ptr_member_of_poly_pointer(base.loc, &deref_type));
            }

            let member_info = env.check_struct_member(deref_type, base.loc, *member)?;

            return Ok(TCAssignTarget {
                kind: TCAssignTargetKind::Ptr(env.buckets.add(base)),
                defn_loc: Some(member_info.loc),
                target_loc: expr.loc,
                target_type: member_info.decl_type,
                offset: member_info.offset,
            });
        }

        ExprKind::Deref(ptr) => {
            let ptr = check_expr(env, local_env, ptr)?;

            let target_type = env.deref(ptr.expr_type, ptr.loc)?;
            return Ok(TCAssignTarget {
                kind: TCAssignTargetKind::Ptr(env.buckets.add(ptr)),
                target_loc: expr.loc,
                defn_loc: None,
                target_type,
                offset: 0,
            });
        }
        ExprKind::BinOp(BinOp::Index, ptr, offset) => {
            let ptr = check_expr(env, local_env, ptr)?;
            let offset = check_expr(env, local_env, offset)?;

            let bin_op = get_overload(env, BinOp::Add, &ptr, &offset)?;
            let map_err = || {
                error!(
                    "invalid operands to index assignment expression",
                    ptr.loc,
                    format!("this has type {}", ptr.expr_type.display(env.files)),
                    offset.loc,
                    format!("this has type {}", offset.expr_type.display(env.files))
                )
            };
            let sum = bin_op.ok_or_else(map_err)?(env, ptr, offset)?;

            let target_type = env.deref(ptr.expr_type, ptr.loc)?;
            return Ok(TCAssignTarget {
                kind: TCAssignTargetKind::Ptr(env.buckets.add(sum)),
                target_loc: expr.loc,
                defn_loc: None,
                target_type,
                offset: 0,
            });
        }

        _ => {
            return Err(error!(
                "expression is not assignable",
                expr.loc, "expression found here"
            ))
        }
    }
}

pub fn main_param_types(loc: CodeLoc) -> Error {
    return error!(
        "can only have param types of (int, char**) or no params for the main function",
        loc, "invalid param types found here"
    );
}

pub fn main_return_type(loc: CodeLoc) -> Error {
    return error!(
        "can only have return type of void or int for the main function",
        loc, "invalid return type found here"
    );
}

pub fn void_variable(loc: CodeLoc) -> Error {
    return error!(
        "cannot define a variable of type void",
        loc, "incorrect variable definition here"
    );
}

pub fn truth_value_of_struct(loc: CodeLoc) -> Error {
    error!(
        "tried to check truth value of struct",
        loc, "this is a struct, when it should be a number or pointer"
    )
}

pub fn ptr_member_of_poly_pointer(ptr_loc: CodeLoc, ptr_type: &TCType) -> Error {
    error!(
        "need to dereference pointer before you can access its members",
        ptr_loc,
        format!(
            "this points to an object of type {:?}, which doesn't have any members",
            ptr_type
        )
    )
}

pub fn member_of_non_struct(loc: CodeLoc) -> Error {
    error!(
        "cannot access member of non-struct",
        loc, "access happened here"
    )
}

pub fn ident_not_found(env: &TypeEnv, loc: CodeLoc) -> Error {
    return error!("couldn't find name", loc, "identifier here");
}

pub fn func_decl_mismatch(original: CodeLoc, new: CodeLoc) -> Error {
    return error!(
        "function declaration type doesn't match previous declaration",
        original, "original declaration here", new, "second declaration here"
    );
}

pub fn func_redef(original: CodeLoc, redef: CodeLoc) -> Error {
    return error!(
        "redefinition of function",
        original, "original definition here", redef, "second definition here"
    );
}

pub fn brace_list(loc: CodeLoc) -> Error {
    error!(
        "brace lists are only allowed when declaring a variable",
        loc, "brace list found here"
    )
}

pub fn array_dimensions_too_high(loc: CodeLoc) -> Error {
    error!(
        "TCI only supports arrays with up to 1 dimensions",
        loc, "array with too many dimensions found here"
    )
}

pub fn type_decl_where_shouldnt_be(loc: CodeLoc) -> Error {
    return error!(
        "type declaration in function return type",
        loc, "found here"
    );
}

pub fn member_incomplete_type(loc: CodeLoc) -> Error {
    error!("member has incomplete type", loc, "member here")
}

pub fn typedef_defined_later(defn: CodeLoc, var: CodeLoc) -> Error {
    return error!(
        "typedef is defined later in the file for non-pointer type (order matters in C)",
        defn, "typedef defined here", var, "typedef referenced here"
    );
}

pub fn typedef_not_defined(loc: CodeLoc) -> Error {
    return error!("typedef is not defined", loc, "referenced here");
}

pub fn struct_defined_later(defn: CodeLoc, var: CodeLoc) -> Error {
    return error!(
        "struct is defined later in the file for non-pointer type (order matters in C)",
        defn, "struct defined here", var, "struct referenced here"
    );
}