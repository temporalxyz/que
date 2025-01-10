#include <inttypes.h>

#include "common.h"
#include "util.h"
#include "shmem.h"

#define CHANNEL_NAME integer
#define CHANNEL_T uint64_t
#define CHANNEL_N 8
#include "consumer.c"

int
main( int argc, char *argv[] ) {
    const char *shmem_id = "shmem";
    size_t buffer_size = sizeof(QUE_(spsc_t)) + CHANNEL_N * sizeof(CHANNEL_T);

    /* Open or create shared memory */
    const char *_page_sz = parse_str_arg( &argc, &argv, "--page-size", "standard" );
    page_size_t page_sz = parse_page_size( _page_sz );
    fprintf( stderr, "opening shmem of size %zu with page size %s\n", buffer_size, _page_sz );
    shmem_t shmem = open_or_create_shmem( shmem_id, buffer_size, page_sz );
    if( !shmem.mem ) {
        return 1;
    }
    fprintf( stderr, "opened shmem\n" );

    /* Cast ptr */
    QUE_(spsc_t) *spsc = (QUE_(spsc_t)*)(shmem.mem);

    /* Print layout */
    fprintf( stderr, "Layout of SPSC\n");
    fprintf( stderr, "tail offset:                %ld\n", ((size_t)(&spsc->tail) - (size_t)spsc));
    fprintf( stderr, "head offset:                %ld\n", ((size_t)(&spsc->head) - (size_t)spsc));
    fprintf( stderr, "producer_heartbeat offset:  %ld\n", ((size_t)(&spsc->producer_heartbeat) - (size_t)spsc));
    fprintf( stderr, "consumer_heartbeat offset:  %ld\n", ((size_t)(&spsc->consumer_heartbeat) - (size_t)spsc));
    fprintf( stderr, "c_padding offset:           %ld\n", ((size_t)(&spsc->c_padding) - (size_t)spsc));
    fprintf( stderr, "padding offset:             %ld\n", ((size_t)(&spsc->padding) - (size_t)spsc));
    fprintf( stderr, "capacity offset:            %ld\n", ((size_t)(&spsc->capacity) - (size_t)spsc));
    fprintf( stderr, "magic offset:               %ld\n", ((size_t)(&spsc->magic) - (size_t)spsc));
}