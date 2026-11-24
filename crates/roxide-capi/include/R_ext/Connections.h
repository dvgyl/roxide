/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef ROXIDE_R_EXT_CONNECTIONS_H
#define ROXIDE_R_EXT_CONNECTIONS_H

#include <Rinternals.h>

#ifdef __cplusplus
extern "C" {
#endif

#define R_CONNECTIONS_VERSION 1

typedef struct Rconn *Rconnection;

Rconnection R_GetConnection(SEXP connection);
SEXP R_new_custom_connection(const char *description, const char *mode,
                             const char *class_name, Rconnection *connection);
size_t R_ReadConnection(Rconnection connection, void *buffer, size_t count);
size_t R_WriteConnection(Rconnection connection, void *buffer, size_t count);

#ifdef __cplusplus
}
#endif

#endif
