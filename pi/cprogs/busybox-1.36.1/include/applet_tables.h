/* This is a generated file, don't edit */

#define NUM_APPLETS 9
#define KNOWN_APPNAME_OFFSETS 0

const char applet_names[] ALIGN1 = ""
"ash" "\0"
"cat" "\0"
"echo" "\0"
"false" "\0"
"ls" "\0"
"mkdir" "\0"
"pwd" "\0"
"sh" "\0"
"true" "\0"
;

#define APPLET_NO_ash 0
#define APPLET_NO_cat 1
#define APPLET_NO_echo 2
#define APPLET_NO_false 3
#define APPLET_NO_ls 4
#define APPLET_NO_mkdir 5
#define APPLET_NO_pwd 6
#define APPLET_NO_sh 7
#define APPLET_NO_true 8

#ifndef SKIP_applet_main
int (*const applet_main[])(int argc, char **argv) = {
ash_main,
cat_main,
echo_main,
false_main,
ls_main,
mkdir_main,
pwd_main,
ash_main,
true_main,
};
#endif

const uint8_t applet_flags[] ALIGN1 = {
0xf0,
0x3e,
0x03,
};

