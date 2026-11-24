/* SPDX-License-Identifier: MIT OR Apache-2.0 */

#include <stddef.h>
#include <stdint.h>

typedef struct SEXPREC *Sexp;

enum { ROX_MAX_NATIVE_ARGS = 65 };

struct RoxDotCallArgs {
    void (*fn)(void);
    int nargs;
    Sexp args[ROX_MAX_NATIVE_ARGS];
    Sexp result;
};

struct RoxDotCArgs {
    void (*fn)(void);
    int nargs;
    void *args[ROX_MAX_NATIVE_ARGS];
};

typedef Sexp (*SexpFn0)(void);
typedef Sexp (*SexpFn1)(Sexp);
typedef Sexp (*SexpFn2)(Sexp, Sexp);
typedef Sexp (*SexpFn3)(Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn4)(Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn5)(Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn6)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn7)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn8)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn9)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn10)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn11)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn12)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn13)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn14)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn15)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn16)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn17)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn18)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn19)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn20)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn21)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn22)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn23)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn24)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn25)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn26)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn27)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn28)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn29)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn30)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn31)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn32)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn33)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn34)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn35)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn36)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn37)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn38)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn39)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn40)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn41)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn42)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn43)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn44)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn45)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn46)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn47)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn48)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn49)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn50)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn51)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn52)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn53)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn54)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn55)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn56)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn57)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn58)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn59)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn60)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn61)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn62)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn63)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn64)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);
typedef Sexp (*SexpFn65)(Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp, Sexp);

typedef void (*PtrFn0)(void);
typedef void (*PtrFn1)(void *);
typedef void (*PtrFn2)(void *, void *);
typedef void (*PtrFn3)(void *, void *, void *);
typedef void (*PtrFn4)(void *, void *, void *, void *);
typedef void (*PtrFn5)(void *, void *, void *, void *, void *);
typedef void (*PtrFn6)(void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn7)(void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn8)(void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn9)(void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn10)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn11)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn12)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn13)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn14)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn15)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn16)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn17)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn18)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn19)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn20)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn21)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn22)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn23)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn24)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn25)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn26)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn27)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn28)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn29)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn30)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn31)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn32)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn33)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn34)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn35)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn36)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn37)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn38)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn39)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn40)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn41)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn42)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn43)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn44)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn45)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn46)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn47)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn48)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn49)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn50)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn51)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn52)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn53)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn54)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn55)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn56)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn57)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn58)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn59)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn60)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn61)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn62)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn63)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn64)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);
typedef void (*PtrFn65)(void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *, void *);

#define A(i) a[i]
#define ARGS1 A(0)
#define ARGS2 ARGS1, A(1)
#define ARGS3 ARGS2, A(2)
#define ARGS4 ARGS3, A(3)
#define ARGS5 ARGS4, A(4)
#define ARGS6 ARGS5, A(5)
#define ARGS7 ARGS6, A(6)
#define ARGS8 ARGS7, A(7)
#define ARGS9 ARGS8, A(8)
#define ARGS10 ARGS9, A(9)
#define ARGS11 ARGS10, A(10)
#define ARGS12 ARGS11, A(11)
#define ARGS13 ARGS12, A(12)
#define ARGS14 ARGS13, A(13)
#define ARGS15 ARGS14, A(14)
#define ARGS16 ARGS15, A(15)
#define ARGS17 ARGS16, A(16)
#define ARGS18 ARGS17, A(17)
#define ARGS19 ARGS18, A(18)
#define ARGS20 ARGS19, A(19)
#define ARGS21 ARGS20, A(20)
#define ARGS22 ARGS21, A(21)
#define ARGS23 ARGS22, A(22)
#define ARGS24 ARGS23, A(23)
#define ARGS25 ARGS24, A(24)
#define ARGS26 ARGS25, A(25)
#define ARGS27 ARGS26, A(26)
#define ARGS28 ARGS27, A(27)
#define ARGS29 ARGS28, A(28)
#define ARGS30 ARGS29, A(29)
#define ARGS31 ARGS30, A(30)
#define ARGS32 ARGS31, A(31)
#define ARGS33 ARGS32, A(32)
#define ARGS34 ARGS33, A(33)
#define ARGS35 ARGS34, A(34)
#define ARGS36 ARGS35, A(35)
#define ARGS37 ARGS36, A(36)
#define ARGS38 ARGS37, A(37)
#define ARGS39 ARGS38, A(38)
#define ARGS40 ARGS39, A(39)
#define ARGS41 ARGS40, A(40)
#define ARGS42 ARGS41, A(41)
#define ARGS43 ARGS42, A(42)
#define ARGS44 ARGS43, A(43)
#define ARGS45 ARGS44, A(44)
#define ARGS46 ARGS45, A(45)
#define ARGS47 ARGS46, A(46)
#define ARGS48 ARGS47, A(47)
#define ARGS49 ARGS48, A(48)
#define ARGS50 ARGS49, A(49)
#define ARGS51 ARGS50, A(50)
#define ARGS52 ARGS51, A(51)
#define ARGS53 ARGS52, A(52)
#define ARGS54 ARGS53, A(53)
#define ARGS55 ARGS54, A(54)
#define ARGS56 ARGS55, A(55)
#define ARGS57 ARGS56, A(56)
#define ARGS58 ARGS57, A(57)
#define ARGS59 ARGS58, A(58)
#define ARGS60 ARGS59, A(59)
#define ARGS61 ARGS60, A(60)
#define ARGS62 ARGS61, A(61)
#define ARGS63 ARGS62, A(62)
#define ARGS64 ARGS63, A(63)
#define ARGS65 ARGS64, A(64)

void rox_capi_call_sexp(void *data) {
    struct RoxDotCallArgs *c = (struct RoxDotCallArgs *)data;
    Sexp *a = c->args;
    switch (c->nargs) {
    case 0: c->result = ((SexpFn0)c->fn)(); break;
#define CASE_SEXP(n) case n: c->result = ((SexpFn##n)c->fn)(ARGS##n); break;
    CASE_SEXP(1)  CASE_SEXP(2)  CASE_SEXP(3)  CASE_SEXP(4)  CASE_SEXP(5)
    CASE_SEXP(6)  CASE_SEXP(7)  CASE_SEXP(8)  CASE_SEXP(9)  CASE_SEXP(10)
    CASE_SEXP(11) CASE_SEXP(12) CASE_SEXP(13) CASE_SEXP(14) CASE_SEXP(15)
    CASE_SEXP(16) CASE_SEXP(17) CASE_SEXP(18) CASE_SEXP(19) CASE_SEXP(20)
    CASE_SEXP(21) CASE_SEXP(22) CASE_SEXP(23) CASE_SEXP(24) CASE_SEXP(25)
    CASE_SEXP(26) CASE_SEXP(27) CASE_SEXP(28) CASE_SEXP(29) CASE_SEXP(30)
    CASE_SEXP(31) CASE_SEXP(32) CASE_SEXP(33) CASE_SEXP(34) CASE_SEXP(35)
    CASE_SEXP(36) CASE_SEXP(37) CASE_SEXP(38) CASE_SEXP(39) CASE_SEXP(40)
    CASE_SEXP(41) CASE_SEXP(42) CASE_SEXP(43) CASE_SEXP(44) CASE_SEXP(45)
    CASE_SEXP(46) CASE_SEXP(47) CASE_SEXP(48) CASE_SEXP(49) CASE_SEXP(50)
    CASE_SEXP(51) CASE_SEXP(52) CASE_SEXP(53) CASE_SEXP(54) CASE_SEXP(55)
    CASE_SEXP(56) CASE_SEXP(57) CASE_SEXP(58) CASE_SEXP(59) CASE_SEXP(60)
    CASE_SEXP(61) CASE_SEXP(62) CASE_SEXP(63) CASE_SEXP(64) CASE_SEXP(65)
#undef CASE_SEXP
    default: c->result = NULL; break;
    }
}

void rox_capi_call_ptrs(void *data) {
    struct RoxDotCArgs *c = (struct RoxDotCArgs *)data;
    void **a = c->args;
    switch (c->nargs) {
    case 0: ((PtrFn0)c->fn)(); break;
#define CASE_PTR(n) case n: ((PtrFn##n)c->fn)(ARGS##n); break;
    CASE_PTR(1)  CASE_PTR(2)  CASE_PTR(3)  CASE_PTR(4)  CASE_PTR(5)
    CASE_PTR(6)  CASE_PTR(7)  CASE_PTR(8)  CASE_PTR(9)  CASE_PTR(10)
    CASE_PTR(11) CASE_PTR(12) CASE_PTR(13) CASE_PTR(14) CASE_PTR(15)
    CASE_PTR(16) CASE_PTR(17) CASE_PTR(18) CASE_PTR(19) CASE_PTR(20)
    CASE_PTR(21) CASE_PTR(22) CASE_PTR(23) CASE_PTR(24) CASE_PTR(25)
    CASE_PTR(26) CASE_PTR(27) CASE_PTR(28) CASE_PTR(29) CASE_PTR(30)
    CASE_PTR(31) CASE_PTR(32) CASE_PTR(33) CASE_PTR(34) CASE_PTR(35)
    CASE_PTR(36) CASE_PTR(37) CASE_PTR(38) CASE_PTR(39) CASE_PTR(40)
    CASE_PTR(41) CASE_PTR(42) CASE_PTR(43) CASE_PTR(44) CASE_PTR(45)
    CASE_PTR(46) CASE_PTR(47) CASE_PTR(48) CASE_PTR(49) CASE_PTR(50)
    CASE_PTR(51) CASE_PTR(52) CASE_PTR(53) CASE_PTR(54) CASE_PTR(55)
    CASE_PTR(56) CASE_PTR(57) CASE_PTR(58) CASE_PTR(59) CASE_PTR(60)
    CASE_PTR(61) CASE_PTR(62) CASE_PTR(63) CASE_PTR(64) CASE_PTR(65)
#undef CASE_PTR
    default: break;
    }
}
