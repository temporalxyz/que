#include "common.h"
#include "util.h"
#include "shmem.h"

#define CHANNEL_NAME integer
#define CHANNEL_T uint64_t
#define CHANNEL_N 4
#include "consumer.c"

int
main( int argc, char *argv[] ) {
    const char *shmem_id = "/shmem";
    size_t buffer_size = sizeof(QUE_(spsc_t)) + CHANNEL_N * sizeof(CHANNEL_T);

    /* Open or create shared memory */
    const char *_page_sz = parse_str_arg( &argc, &argv, "--page-size", "standard" );
    page_size_t page_sz = parse_page_size( _page_sz );
    fprintf( stderr, "opening shmem of size %ld with page size %s\n", buffer_size, _page_sz );
    shmem_t shmem = open_or_create_shmem( shmem_id, buffer_size, page_sz );
    if( !shmem.mem ) {
        return 1;
    }
    fprintf( stderr, "opened shmem\n" );


    /* Join as consumer (must be initialized already) */
    CONSUMER_(consumer_t) consumer;
    fprintf( stderr, "joining consumer\n" );
    if( CONSUMER_(join)( shmem.mem, &consumer ) ) {
        fprintf( stderr, "Failed to join consumer\n" );
        close_shmem( shmem );
        return 1;
    }
    fprintf( stderr, "joined consumer. magic %ld; capacity %ld\n", consumer.spsc->magic, consumer.spsc->capacity );


    /* ack join */
    CONSUMER_(beat)( &consumer );

    /* read value */
    CHANNEL_T value;
    while( CONSUMER_(pop)( &consumer, &value ) ) {
        /* wait for producer to publish */
    }
    fprintf( stderr, "read value %ld\n", value );

    /* ack message */
    CONSUMER_(beat)( &consumer );

    /* Clean up */
    close_shmem( shmem );
}