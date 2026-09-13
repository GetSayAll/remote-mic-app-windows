#include "remap.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

int main(void)
{
    unsigned int id, usage;
    size_t length;
    unsigned char report[121], expected[121];
    unsigned long cases = 0;
    assert(!SayAllThreeButtonRemapReport(NULL, 121));
    for (id = 0; id < 256; ++id) {
        for (usage = 0; usage < 256; ++usage) {
            for (length = 0; length <= sizeof(report); ++length) {
                int changed = 0;
                memset(report, 0xa5, sizeof(report));
                report[0] = (unsigned char)id;
                report[3] = (unsigned char)usage;
                memcpy(expected, report, sizeof(report));
                if (id == 1 && length >= 4) {
                    if (usage == 0x80) { expected[3] = 0x68; changed = 1; }
                    if (usage == 0x81) { expected[3] = 0x69; changed = 1; }
                    if (usage == 0xf1) { expected[3] = 0x6a; changed = 1; }
                }
                assert(SayAllThreeButtonRemapReport(report, length) == changed);
                assert(memcmp(report, expected, sizeof(report)) == 0);
                ++cases;
            }
        }
    }
    /* A press/repeat/release sequence keeps release and voice F5 intact. */
    memset(report, 0, sizeof(report));
    report[0] = 1;
    report[3] = 0x80;
    assert(SayAllThreeButtonRemapReport(report, sizeof(report)));
    assert(!SayAllThreeButtonRemapReport(report, sizeof(report)));
    report[3] = 0;
    assert(!SayAllThreeButtonRemapReport(report, sizeof(report)));
    report[3] = 0x3e;
    assert(!SayAllThreeButtonRemapReport(report, sizeof(report)));
    puts("PASS: release, repeat, voice unchanged");
    printf("PASS: %lu exhaustive report boundary cases\n", cases);
    return 0;
}
