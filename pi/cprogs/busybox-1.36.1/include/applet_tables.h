/* This is a generated file, don't edit */

#define NUM_APPLETS 6
#define KNOWN_APPNAME_OFFSETS 0

const char applet_names[] ALIGN1 = ""
"ash" "\0"
"cat" "\0"
"fatattr" "\0"
"ls" "\0"
"sh" "\0"
"vi" "\0"
;

#define APPLET_NO_ash 0
#define APPLET_NO_cat 1
#define APPLET_NO_fatattr 2
#define APPLET_NO_ls 3
#define APPLET_NO_sh 4
#define APPLET_NO_vi 5

#ifndef SKIP_applet_main
int (*const applet_main[])(int argc, char **argv) = {
ash_main,
cat_main,
fatattr_main,
ls_main,
ash_main,
vi_main,
};
#endif

const uint8_t applet_flags[] ALIGN1 = {
0xa0,
0x00,
};

