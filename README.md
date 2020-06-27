# Teaching C Compiler
The goals of this compiler are:

1. Provide better error messages for new programmers
2. Generate code that checks for segmentation faults at runtime and provides debug
   information
3. Output warnings when the user is doing something they shouldn't be.


## Building this Project
Run `gcc src/*.c` to build, and then `./a.out` to run the program.

## Restrictions and Incompatibilities

- Casting in the C style is not supported. Instead, a builtin `cast(expr, type)`
  is provided.
- Anonymous structs aren't supported.
- Implicit types on functions aren't supported.
- Implicit function declarations aren't supported.
- Type names must start with an uppercase leter, and variable names must start
  with a lowercase letter.
- Higher order functions without a typedef are not supported without
- Declarations can only be done using `declare(type, name)` for normal
  variables and `declare_array(type, name)` for arrays.
