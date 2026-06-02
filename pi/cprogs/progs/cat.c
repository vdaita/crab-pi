
#include <unistd.h>
#include <errno.h>
#include <stdlib.h>

int main(void) {
	const size_t BUF_SZ = 8192;
	char *buf = malloc(BUF_SZ);
	if (!buf) return 2;

	for (;;) {
		ssize_t r = read(STDIN_FILENO, buf, BUF_SZ);
		if (r == 0) break; /* EOF */
		if (r < 0) {
			/* Read error */
			free(buf);
			return 1;
		}
		char *p = buf;
		ssize_t to_write = r;
		while (to_write > 0) {
			ssize_t w = write(STDOUT_FILENO, p, to_write);
			if (w < 0) {
				/* Write error */
				free(buf);
				return 1;
			}
			p += w;
			to_write -= w;
		}
	}

	free(buf);
	return 0;
}
