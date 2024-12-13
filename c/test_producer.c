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
    fprintf( stderr, "opening shmem of size %ld with page size %s\n", buffer_size, _page_sz );
    shmem_t shmem = open_or_create_shmem( shmem_id, buffer_size, page_sz );
    if( !shmem.mem ) {
        return 1;
    }
    fprintf( stderr, "mapped shmem\n" );

    /* Initialize producer */
    PRODUCER_(producer_t) producer;
    fprintf( stderr, "initializing producer\n" );
    memset( shmem.mem, 0, buffer_size );
    if( PRODUCER_(initialize_in)( shmem.mem, &producer ) ) {
        fprintf( stderr, "Failed to initialize producer\n" );
        close_shmem( shmem );
        return 1;
    }
    fprintf( stderr, "initialized producer. magic %ld; capacity %ld\n", producer.spsc->magic, producer.spsc->capacity );

    /* Wait for consumer to ack join */
    while( !consumer_heartbeat( &producer ) ) {}

    /* Push value */
    uint64_t value;
    memset( (unsigned char *)&value, 42, sizeof(value) );
    for( int i = 0; i < 4; i++ ) {
        PRODUCER_(push_lossless)( &producer, &value );
    }
    fprintf( stderr, "pushed value %ld \n", value);

    /* lossless push should fail*/
    assert( PRODUCER_(push_lossless)( &producer, &value ) == 1 ); 

    /* Sync the producer */
    sync_producer( &producer );
    PRODUCER_(beat)( &producer );
    fprintf( stderr, "published value\n" );

    /* Wait for consumer to ack message */
    while( !consumer_heartbeat( &producer ) ) {}

    /* Clean up */
    close_shmem( shmem );
    fprintf( stderr, "cleanup done\n" );


    return 0;
}
