#include "util.h"
#include "common.h"

/* Parses the command-line argument and returns its value, or a default
   if not found */
const char*
parse_str_arg( int *argc, char ***argv, const char *option, const char *default_value )
{
    for( int i = 1; i < *argc; i++ )
    {
        if( strcmp( (*argv)[i], option ) == 0 && i + 1 < *argc )
        {
            const char *value = (*argv)[i + 1];

            for( int j = i; j < *argc - 2; j++ )
            {
                (*argv)[j] = (*argv)[j + 2];
            }
            *argc -= 2;
            return value;
        }
    }
    return default_value;
}

/* Parses the command-line argument and returns its value, or a default
   if not found */
uint64_t
parse_ulong_arg( int *argc, char ***argv, const char *option, uint64_t default_value )
{
    for( int i = 1; i < *argc; i++ )
    {
        if( strcmp( (*argv)[i], option ) == 0 && i + 1 < *argc )
        {
            const char *arg_value = (*argv)[i + 1];
            char *endptr;
            errno = 0;
            uint64_t int_value = strtol( arg_value, &endptr, 10 );

            // Check for errors during conversion
            if( errno != 0 || *endptr != '\0' )
            {
                fprintf( stderr, "Invalid integer for option %s: %s\n", option, arg_value );
                exit( EXIT_FAILURE );
            }

            // Shift the remaining arguments over the found ones (removes them from argv)
            for( int j = i; j < *argc - 2; j++ )
            {
                (*argv)[j] = (*argv)[j + 2];
            }
            *argc -= 2;  // Reduce the argument count
            return (uint64_t) int_value;
        }
    }
    return default_value;
}
/* Function to convert string to page size */
page_size_t
parse_page_size( const char *arg )
{
    if( strcmp( arg, "standard" ) == 0 )
    {
        return STANDARD_PAGE;
    }
#if __linux__
    else if( strcmp( arg, "huge" ) == 0 )
    {
        return HUGE_PAGE_2MB;
    }
    else if( strcmp( arg, "gigantic" ) == 0 )
    {
        return GIGANTIC_PAGE_1GB;
    }
#endif
    else
    {
        fprintf( stderr, "Invalid page size: %s\n", arg );
        exit( EXIT_FAILURE );
    }
}