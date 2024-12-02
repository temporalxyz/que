#ifndef QUE_UTIL_H
#define QUE_UTIL_H

#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <errno.h>

#include "common.h"


/* Parses the command-line argument and returns its value, or a default
   if not found */
const char*
parse_str_arg( int *argc, char ***argv, const char *option, const char *default_value );

/* Parses the command-line argument and returns its value, or a default
   if not found */
uint64_t
parse_ulong_arg( int *argc, char ***argv, const char *option, uint64_t default_value );

/* Function to convert string to page size */
page_size_t
parse_page_size( const char *arg );



#endif