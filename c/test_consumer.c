#include <inttypes.h>

#include "common.h"
#include "util.h"
#include "shmem.h"

#define CHANNEL_NAME integer
#define CHANNEL_T uint64_t
#define CHANNEL_N 4
#include "consumer.c"

int
main( int argc, char *argv[] ) {
    const char *shmem_id = "shmem";
    size_t buffer_size = sizeof(QUE_(spsc_t)) + CHANNEL_N * sizeof(CHANNEL_T);

    /* Open or create shared memory */
    const char *_page_sz = parse_str_arg( &argc, &argv, "--page-size", "standard" );
    page_size_t page_sz = parse_page_size( _page_sz );
    fprintf( stderr, "opening shmem of size %zu with page size %s=%lu\n", buffer_size, _page_sz, page_sz );
    shmem_t shmem = open_or_create_shmem( shmem_id, buffer_size, page_sz );
    if( !shmem.mem ) {
        return 1;
    }
    fprintf( stderr, "opened shmem\n" );


    /* Join as consumer (must be initialized already) */
    integer_consumer_consumer_t consumer;
    fprintf( stderr, "joining consumer\n" );
    if( integer_consumer_join( shmem.mem, &consumer ) ) {
        fprintf( stderr, "Failed to join consumer\n" );
        close_shmem( shmem );
        return 1;
    }
    fprintf( stderr, "joined consumer. magic %" PRIu64 "; capacity %" PRIu64 "\n", consumer.spsc->magic, consumer.spsc->capacity );


    /* ack join */
    fprintf( stderr, "sent consumer ack 1\n" );
    integer_consumer_beat( &consumer );

    /* read value */
    CHANNEL_T value;
    for( int i=0; i<4; i++){
        while( integer_consumer_pop( &consumer, &value ) ) {
            /* wait for producer to publish */
        }
        fprintf( stderr, "read value %" PRIu64 "\n", value );
    }

    /* ack message */
    integer_consumer_beat( &consumer );
    fprintf( stderr, "sent consumer ack 2\n" );
    
    fprintf( stderr, "waiting for producer ack\n" );
    while( !integer_consumer_producer_heartbeat( &consumer ) ) {}
    
    /* read value */
    fprintf( stderr, "reading value\n" );
    while( integer_consumer_pop( &consumer, &value ) ) {
        /* wait for producer to publish */
    }
    fprintf( stderr, "read value %" PRIu64 "\n", value );

    /* ack message */
    integer_consumer_beat( &consumer );
    fprintf( stderr, "sent consumer ack 3\n" );


    /* Clean up */
    close_shmem( shmem );
    fprintf( stderr, "done\n\n" );
}