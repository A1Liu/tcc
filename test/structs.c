struct FileId {
  int ident;
  int length;
  int cursor;
};

int printf(char* format, ...);
int ident_of(struct FileId file) { return file.ident; }

int main(int argc, char **argv) {
  struct FileId file;
  struct FileId file2;
  struct FileId * file3;
  file3 = &file2;
  file.ident = 12;
  file2.ident = ident_of(file);

  printf("%d\n", file3->ident);
  file3->ident = 13;
  printf("%d\n", file2);
}