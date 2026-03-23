#include <inttypes.h>

#include "common.h"
#include "util.h"
#include "shmem.h"


#define CHANNEL_NAME integer
#define CHANNEL_T uint64_t
#define CHANNEL_N 4
#include "producer.c"

int
main( int argc, char *argv[] ) {
    const char *shmem_id = "shmem";
    shm_unlink( shmem_id );
    size_t buffer_size = sizeof(QUE_(spsc_t)) + CHANNEL_N * sizeof(CHANNEL_T);

    /* Open or create shared memory */
    const char *_page_sz = parse_str_arg( &argc, &argv, "--page-size", "standard" );
    page_size_t page_sz = parse_page_size( _page_sz );
    fprintf( stderr, "opening shmem of size %zu with page size %s=%lu\n", buffer_size, _page_sz, page_sz);
    shmem_t shmem = open_or_create_shmem( shmem_id, buffer_size, page_sz );
    if( !shmem.mem ) {
        return 1;
    }
    fprintf( stderr, "mapped shmem\n" );

    /* Initialize producer */
   integer_producer_producer_t producer;
    fprintf( stderr, "initializing producer\n" );
    memset( shmem.mem, 0, buffer_size );
    if( integer_producer_join_or_initialize( shmem.mem, &producer ) ) {
        fprintf( stderr, "Failed to initialize producer\n" );
        close_shmem( shmem );
        return 1;
    }
    fprintf( stderr, "initialized producer. magic %" PRIu64 "; capacity %" PRIu64 "\n", producer.spsc->magic, producer.spsc->capacity );

    /* Wait for consumer to ack join */
    fprintf( stderr, "waiting for consumer ack 1\n" );
    while( !integer_producer_consumer_heartbeat( &producer ) ) {}

    /* Push value */
    fprintf( stderr, "pushing 4 values\n" );
    uint64_t value = 42;
    for( int i = 0; i < 4; i++ ) {
       integer_producer_push( &producer, &value );
    }
    fprintf( stderr, "pushed 4 values %" PRIu64 "\n", value);

    /* lossless push should fail*/
    assert(integer_producer_push( &producer, &value ) == 1 ); 

    /* Sync the producer */
    integer_producer_sync( &producer );
    fprintf( stderr, "published value\n" );

    /* Wait for consumer to ack message */
    fprintf( stderr, "waiting for consumer ack 2\n" );
    while( !integer_producer_consumer_heartbeat( &producer ) ) {}
    fprintf( stderr, "sending ack 1\n" );
    integer_producer_beat( &producer );

    /* Now via reserve/commit semantics */
    integer_producer_reservation_t res[1];
    fprintf( stderr, "reserving\n" );
    while( integer_producer_reserve( &producer, 1, res ) ) {};
    uint64_t * slot = integer_producer_reservation_get_next( res );
    *slot = 420;
    integer_producer_reservation_write_next( res );
    integer_producer_reservation_commit( res );
    printf("reserve/commit published 420\n");

    /* Wait for consumer to ack message */
    fprintf( stderr, "waiting for consumer ack 3\n" );
    while( !integer_producer_consumer_heartbeat( &producer ) ) {}

    /* Clean up */
    close_shmem( shmem );
    fprintf( stderr, "done\n\n" );


    return 0;
}
