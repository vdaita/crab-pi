/* This is a generated file, don't edit */

#define NUM_APPLETS 1
#define SINGLE_APPLET_STR "hush"
#define SINGLE_APPLET_MAIN hush_main
#define KNOWN_APPNAME_OFFSETS 0

const char applet_names[] ALIGN1 = ""
"hush" "\0"
;

#define APPLET_NO_hush 0

#ifndef SKIP_applet_main
int (*const applet_main[])(int argc, char **argv) = {
hush_main,
};
#endif

const uint8_t applet_suid[] ALIGN1 = {
0x00,
};

