#include <cstdio>
#include "math.h"

int main() {
  if (add(2, 2) != 4) {
    std::fprintf(stderr, "add(2,2) != 4\n");
    return 1;
  }
  return 0;
}
