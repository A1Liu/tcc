
int printf(char *format, ...);

int main(int argc, char **argv) {
  int target = 0, *hello = &target, *goodbye = &target;

  printf("%d\n", *goodbye);
  *goodbye = 12;
  printf("%d\n", *hello);
  *hello = 13;
  printf("%d\n", target);
}
