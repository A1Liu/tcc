use crate::*;

#[derive(Debug, Clone, PartialEq, Hash, Eq, Copy)]
pub enum BinOp {
    Add,
    Sub,
    Assign,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind<'a> {
    IntLiteral(i32),
    CharLiteral(u8),
    StringLiteral(&'a str),
    Ident(u32),
    BinOp(BinOp, &'a Expr<'a>, &'a Expr<'a>),
    Call {
        function: &'a Expr<'a>,
        params: &'a [Expr<'a>],
    },
    Member {
        base: &'a Expr<'a>,
        member: u32,
    },
    PtrMember {
        base: &'a Expr<'a>,
        member: u32,
    },
    Index {
        ptr: &'a Expr<'a>,
        index: &'a Expr<'a>,
    },
    List(&'a [Expr<'a>]),
    PostIncr(&'a Expr<'a>),
    PostDecr(&'a Expr<'a>),
    Ref(&'a Expr<'a>),
    Deref(&'a Expr<'a>),
    Uninit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr<'a> {
    pub kind: ExprKind<'a>,
    pub range: Range,
}

#[derive(Debug)]
pub struct InnerStructDecl {
    pub decl_type: ASTType,
    pub pointer_count: u32,
    pub ident: u32,
    pub range: Range,
}

#[derive(Debug)]
pub enum ParamKind {
    StructLike {
        decl_type: ASTType,
        pointer_count: u32,
        ident: u32,
    },
    Vararg,
}

#[derive(Debug)]
pub struct ParamDecl {
    pub kind: ParamKind,
    pub range: Range,
}

#[derive(Debug)]
pub struct StructDecl<'a> {
    pub ident: u32,
    pub ident_range: Range,
    pub members: Option<&'a [InnerStructDecl]>,
    pub range: Range,
}

#[derive(Debug, Clone)]
pub struct Decl<'a> {
    pub pointer_count: u32,
    pub ident: u32,
    pub range: Range,
    pub expr: Expr<'a>,
}

#[derive(Debug)]
pub enum GlobalStmtKind<'a> {
    Func {
        return_type: ASTType,
        pointer_count: u32,
        ident: u32,
        params: &'a [ParamDecl],
        body: &'a [Stmt<'a>],
    },
    FuncDecl {
        return_type: ASTType,
        pointer_count: u32,
        ident: u32,
        params: &'a [ParamDecl],
    },
    StructDecl(StructDecl<'a>),
    Decl {
        decl_type: ASTType,
        decls: &'a [Decl<'a>],
    },
}

#[derive(Debug)]
pub struct GlobalStmt<'a> {
    pub kind: GlobalStmtKind<'a>,
    pub range: Range,
}

#[derive(Debug)]
pub enum ASTTypeKind {
    Int,
    Struct { ident: u32 },
    Char,
    Void,
}

#[derive(Debug)]
pub struct ASTType {
    pub kind: ASTTypeKind,
    pub range: Range,
}

#[derive(Debug)]
pub enum StmtKind<'a> {
    Decl {
        decl_type: ASTType,
        decls: &'a [Decl<'a>],
    },
    Expr(Expr<'a>),
    Nop,
    Ret,
    RetVal(Expr<'a>),
    Branch {
        if_cond: Expr<'a>,
        if_body: &'a [Stmt<'a>],
        else_body: Option<&'a [Stmt<'a>]>,
    },
    Block(&'a [Stmt<'a>]),
    For {
        at_start: Expr<'a>,
        condition: Expr<'a>,
        post_expr: Expr<'a>,
        body: &'a [Stmt<'a>],
    },
    ForDecl {
        at_start_decl_type: ASTType,
        at_start: &'a [Decl<'a>],
        condition: Expr<'a>,
        post_expr: Expr<'a>,
        body: &'a [Stmt<'a>],
    },
    While {
        condition: Expr<'a>,
        body: &'a [Stmt<'a>],
    },
}

#[derive(Debug)]
pub struct Stmt<'a> {
    pub kind: StmtKind<'a>,
    pub range: Range,
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub struct SizeAlign {
    pub size: u32,
    pub align: u32,
}

pub fn sa(size: u32, align: u32) -> SizeAlign {
    SizeAlign { size, align }
}

pub const TC_UNKNOWN_SA: SizeAlign = SizeAlign {
    size: TC_UNKNOWN_SIZE,
    align: 0,
};

#[derive(Debug, Clone, PartialEq)]
pub struct TCStructMember {
    pub decl_type: TCType,
    pub ident: u32,
    pub loc: CodeLoc,
    pub offset: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TCStructDefn<'a> {
    pub defn_idx: u32,
    pub members: &'a [TCStructMember],
    pub loc: CodeLoc,
    pub sa: SizeAlign,
}

#[derive(Debug, PartialEq)]
pub struct TCStruct<'a> {
    pub decl_idx: u32,
    pub defn: Option<TCStructDefn<'a>>,
    pub decl_loc: CodeLoc,
}

pub const TC_UNKNOWN_SIZE: u32 = !0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TCTypeKind {
    I32, // int
    U64, // unsigned long
    Char,
    Void,
    Struct { ident: u32, sa: SizeAlign },
    Uninit { size: u32 },
}

#[derive(Debug, Clone, Copy)]
pub struct TCType {
    pub kind: TCTypeKind,
    pub pointer_count: u32,
}

#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub enum TCShallowType {
    I32, // int
    U64, // unsigned long
    Char,
    Void,
    Struct,
    Pointer,
}

#[derive(Debug, Clone)]
pub struct TCVar {
    pub decl_type: TCType,
    pub var_offset: i16, // TODO TCVarKind with global and local
    pub loc: CodeLoc,    // we allow extern in include files so the file is not known apriori
}

#[derive(Debug, Clone)]
pub struct TCFuncParam {
    pub decl_type: TCType,
    pub ident: u32,
    pub range: Range,
}

#[derive(Debug, Clone, Copy)]
pub struct TCFuncType<'a> {
    pub decl_idx: u32,
    pub return_type: TCType,
    pub loc: CodeLoc,
    pub params: &'a [TCFuncParam],
    pub varargs: bool,
}

#[derive(Debug, Clone)]
pub struct TCFuncDefn<'a> {
    pub defn_idx: u32,
    pub loc: CodeLoc,
    pub stmts: &'a [TCStmt<'a>],
}

#[derive(Debug, Clone)]
pub struct TCFunc<'a> {
    pub func_type: TCFuncType<'a>,
    pub defn: Option<TCFuncDefn<'a>>,
}

#[derive(Debug, Clone)]
pub enum TCAssignKind<'a> {
    LocalIdent { var_offset: i16 },
    Ptr(&'a TCExpr<'a>),
}

#[derive(Debug, Clone)]
pub struct TCAssignTarget<'a> {
    pub kind: TCAssignKind<'a>,
    pub defn_loc: Option<CodeLoc>,
    pub target_range: Range,
    pub target_type: TCType,
    pub offset: u32,
}

#[derive(Debug, Clone)]
pub enum TCStmtKind<'a> {
    RetVal(TCExpr<'a>),
    Ret,
    Expr(TCExpr<'a>),
    Decl { init: TCExpr<'a> },
}

#[derive(Debug, Clone)]
pub struct TCStmt<'a> {
    pub kind: TCStmtKind<'a>,
    pub range: Range,
}

#[derive(Debug, Clone)]
pub enum TCExprKind<'a> {
    Uninit,
    IntLiteral(i32),
    StringLiteral(&'a str),
    LocalIdent {
        var_offset: i16,
    },

    AddI32(&'a TCExpr<'a>, &'a TCExpr<'a>),
    AddU64(&'a TCExpr<'a>, &'a TCExpr<'a>),

    SubI32(&'a TCExpr<'a>, &'a TCExpr<'a>),

    SConv8To32(&'a TCExpr<'a>),
    SConv32To64(&'a TCExpr<'a>),

    ZConv8To32(&'a TCExpr<'a>),
    ZConv32To64(&'a TCExpr<'a>),

    Assign {
        target: TCAssignTarget<'a>,
        value: &'a TCExpr<'a>,
    },

    Member {
        base: &'a TCExpr<'a>,
        offset: u32,
    },

    Deref(&'a TCExpr<'a>),
    Ref(TCAssignTarget<'a>),

    Call {
        func: u32,
        params: &'a [TCExpr<'a>],
        varargs: bool,
    },
}

#[derive(Debug, Clone)]
pub struct TCExpr<'a> {
    pub kind: TCExprKind<'a>,
    pub expr_type: TCType,
    pub range: Range,
}

impl TCType {
    pub fn to_shallow(&self) -> TCShallowType {
        if self.pointer_count > 0 {
            return TCShallowType::Pointer;
        }

        match self.kind {
            TCTypeKind::I32 => TCShallowType::I32,
            TCTypeKind::U64 => TCShallowType::U64,
            TCTypeKind::Char => TCShallowType::Char,
            TCTypeKind::Void => TCShallowType::Void,
            TCTypeKind::Struct { .. } => TCShallowType::Struct,
            TCTypeKind::Uninit { .. } => panic!("cannot make shallow of uninit"),
        }
    }

    #[inline]
    pub fn size(&self) -> u32 {
        if self.pointer_count > 0 {
            return 8;
        }

        match self.kind {
            TCTypeKind::U64 => 8,
            TCTypeKind::I32 => 4,
            TCTypeKind::Char => 1,
            TCTypeKind::Void => 0,
            TCTypeKind::Struct { sa, .. } => {
                debug_assert!(sa.size != TC_UNKNOWN_SIZE);
                sa.size
            }
            TCTypeKind::Uninit { size } => size,
        }
    }

    #[inline]
    pub fn align(&self) -> u32 {
        if self.pointer_count > 0 {
            return 8;
        }

        match self.kind {
            TCTypeKind::U64 => 8,
            TCTypeKind::I32 => 4,
            TCTypeKind::Char => 1,
            TCTypeKind::Void => 0,
            TCTypeKind::Struct { sa, .. } => {
                debug_assert!(sa.size != TC_UNKNOWN_SIZE);
                sa.align
            }
            TCTypeKind::Uninit { size } => size,
        }
    }
}

impl PartialEq for TCType {
    fn eq(&self, other: &Self) -> bool {
        return self.kind == other.kind && self.pointer_count == other.pointer_count;
    }
}

impl<'a> PartialEq for TCFuncType<'a> {
    fn eq(&self, other: &Self) -> bool {
        if self.return_type != other.return_type {
            return false;
        }

        if self.params.len() != other.params.len() {
            return false;
        }

        for i in 0..self.params.len() {
            if self.params[i].decl_type != other.params[i].decl_type {
                return false;
            }
        }

        return true;
    }
}
