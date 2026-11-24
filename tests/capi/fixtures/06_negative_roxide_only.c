/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#if defined(FIXTURE_FOREIGN_STUB)
int fixture_foreign_symbol(void)
{
    return 1;
}
#elif defined(FIXTURE_FOREIGN_CLIENT)
#include <stdio.h>
#include <stdlib.h>

extern int fixture_foreign_symbol(void);

__attribute__((constructor)) static void record_load(void)
{
    if (fixture_foreign_symbol()) {
        const char *marker = getenv("ROXIDE_FIXTURE_LOAD_MARKER");
        if (marker) {
            FILE *file = fopen(marker, "w");
            if (file)
                fclose(file);
        }
    }
}
#else
#include <Rinternals.h>

void fixture_overrun(unsigned char *buffer)
{
    buffer[1] ^= 0xff;
    buffer[64] ^= 0xff;
}

void fixture_underrun(unsigned char *buffer)
{
    buffer[-1] ^= 0xff;
    buffer[-64] ^= 0xff;
}
#endif
