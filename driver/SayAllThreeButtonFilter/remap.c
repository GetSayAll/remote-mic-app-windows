#include "remap.h"

/* Adapted from QL-4/RemoteMapper, see LICENSE.RemoteMapper and ATTRIBUTION.md.
 * RC001/RC003 keyboard collection: ID, modifiers, reserved, single usage.
 * Do not scan padding or vendor/voice reports as keyboard slots.
 */
int SayAllThreeButtonRemapReport(unsigned char *report, size_t length)
{
    if (report == NULL || length < 4 || report[0] != 1) {
        return 0;
    }
    switch (report[3]) {
    case 0x80: report[3] = 0x68; return 1; /* Volume+ -> F13 */
    case 0x81: report[3] = 0x69; return 1; /* Volume- -> F14 */
    case 0xf1: report[3] = 0x6a; return 1; /* Back -> F15 */
    default: return 0;
    }
}
