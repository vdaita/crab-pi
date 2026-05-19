cmd_scripts/kconfig/lxdialog/textbox.o := gcc -Wp,-MD,scripts/kconfig/lxdialog/.textbox.o.d -Wall -Wstrict-prototypes -O2 -fomit-frame-pointer   -DNCURSES_WIDECHAR -DCURSES_LOC="<ncurses.h>" -DNCURSES_WIDECHAR=1 -DLOCALE    -c -o scripts/kconfig/lxdialog/textbox.o scripts/kconfig/lxdialog/textbox.c

deps_scripts/kconfig/lxdialog/textbox.o := \
  scripts/kconfig/lxdialog/textbox.c \
  /usr/include/stdc-predef.h \
  scripts/kconfig/lxdialog/dialog.h \
  /usr/include/sys/types.h \
  /usr/include/features.h \
  /usr/include/bits/alltypes.h \
  /usr/include/endian.h \
  /usr/include/fortify/sys/select.h \
  /usr/include/sys/select.h \
  /usr/include/fcntl.h \
  /usr/include/bits/fcntl.h \
  /usr/include/fortify/unistd.h \
  /usr/include/unistd.h \
  /usr/include/bits/posix.h \
  /usr/include/fortify/fortify-headers.h \
  /usr/include/ctype.h \
  /usr/include/fortify/stdlib.h \
  /usr/include/stdlib.h \
  /usr/include/alloca.h \
  /usr/include/limits.h \
  /usr/include/bits/limits.h \
  /usr/include/fortify/string.h \
  /usr/include/string.h \
  /usr/include/fortify/strings.h \
  /usr/include/strings.h \
  /usr/include/curses.h \
  /usr/include/ncurses_dll.h \
  /usr/include/stdint.h \
  /usr/include/bits/stdint.h \
  /usr/include/fortify/stdio.h \
  /usr/include/stdio.h \
  /usr/include/stdarg.h \
  /usr/include/stddef.h \
  /usr/include/stdbool.h \
  /usr/include/fortify/wchar.h \
  /usr/include/wchar.h \
  /usr/include/unctrl.h \
  /usr/include/curses.h \

scripts/kconfig/lxdialog/textbox.o: $(deps_scripts/kconfig/lxdialog/textbox.o)

$(deps_scripts/kconfig/lxdialog/textbox.o):
