#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char **argv) {
    FILE *f = fopen("work.txt", "a");
    if (!f) {
        perror("fake-agent: fopen(work.txt)");
        return 1;
    }
    fputs("modified by fake-agent in sandbox\n", f);
    fclose(f);
    printf("fake-agent: modified work.txt successfully\n");
    return 0;
}
