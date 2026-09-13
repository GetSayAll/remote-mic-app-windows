#pragma once
#include <stddef.h>

/* Pure report transform shared by kernel driver and user-mode contract tests. */
int SayAllThreeButtonRemapReport(unsigned char *report, size_t length);
