#include <stdio.h>
#include <stdint.h>
#include <string.h>
#include <assert.h>
#include <stdatomic.h>
#include <stddef.h>

#include "spsc.c"


#define PRODUCER_CONCAT3(a, b, c) a##b##c
#define PRODUCER_EXPAND_THEN_CONCAT3(a, b, c) PRODUCER_CONCAT3(a, b, c)
#define PRODUCER_(x) PRODUCER_EXPAND_THEN_CONCAT3(CHANNEL_NAME, _producer_, x)


typedef struct PRODUCER_(producer) {
    QUE_(spsc_t) * spsc;         /* Pointer to shared SPSC */
    uint64_t tail;                    /* Local tail */
    uint64_t written;                 /* Number of written elements since last sync */
    uint64_t last_consumer_heartbeat; /* Last known consumer heartbeat */
} PRODUCER_(producer_t);


static inline int
PRODUCER_(initialize_in)( void *shmem_region, PRODUCER_(producer_t)* producer ) {

    /* Capacity must be a power of two */
    assert( CHANNEL_N > 0 && (CHANNEL_N & (CHANNEL_N - 1)) == 0 );  

    /* Ensure alignment */
    assert( ((size_t)shmem_region % ALIGNMENT) == 0 ); 

    QUE_(spsc_t) *spsc = (QUE_(spsc_t) *)shmem_region;

    /* Join existing queue */
    if( spsc->magic == MAGIC ) {
        if( spsc->capacity != CHANNEL_N ) {
            fprintf( stderr, "Incorrect capacity\n" );
            return 1;
        }

        producer->spsc = spsc;
        producer->tail = atomic_load_explicit( &(spsc->tail), memory_order_acquire );
        producer->written = 0;
        producer->last_consumer_heartbeat = atomic_load_explicit( &(spsc->consumer_heartbeat),
                                                                  memory_order_acquire );

        return 0;
    }
    
    /* Initialization. In theory this could be corrupted... */
    if( spsc->magic == 0 ) {
        /* Initialize the SPSC structure */
        atomic_store_explicit( &(spsc->tail), 0, memory_order_release );
        /* no head initalization. consumer will update head when joining */
        atomic_store_explicit( &(spsc->producer_heartbeat), 0, memory_order_release );
        spsc->capacity = CHANNEL_N;
        spsc->magic = MAGIC;

        producer->spsc = spsc;
        producer->tail = 0;
        producer->written = 0;
        producer->last_consumer_heartbeat = atomic_load_explicit( &(spsc->consumer_heartbeat), 
                                                                  memory_order_acquire );

        return 0;
    }

    fprintf( stderr, "Corruption detected\n" );
    return 1;
}


    
static inline void
PRODUCER_(push)( PRODUCER_(producer_t) *producer, void *value ) {
    QUE_(spsc_t) *spsc = producer->spsc;

    if( producer->written == burst_amount(CHANNEL_N) ) {
        /* Sync the written tail */
        atomic_store_explicit( &(spsc->tail), producer->tail, memory_order_release );
        producer->written = 0;
    }

    /* Calculate the index in the circular buffer */
    size_t index = producer->tail & (CHANNEL_N - 1);

    /* Calculate the base address of the buffer (assume it follows the SPSC struct in memory) */
    void *buffer_base = (char *)spsc + sizeof(QUE_(spsc_t)) - 112;
    size_t misalignment = (size_t)(buffer_base) % __alignof__(CHANNEL_T);
    size_t adjustment = (__alignof__(CHANNEL_T) - misalignment) & -(misalignment != 0);
    buffer_base += adjustment;

    /* Calculate the address in the buffer by offsetting the index by value_size */
    void *addr = (char *)buffer_base + (index * sizeof(CHANNEL_T));

    /* Write the value to the buffer using memcpy */
    memcpy( addr, value, sizeof(CHANNEL_T) );

    /* Increment tail and written counter */
    producer->tail++;
    producer->written++;
}

static inline int
PRODUCER_(push_lossless)( PRODUCER_(producer_t) *producer, void *value ) {
    QUE_(spsc_t) *spsc = producer->spsc;

    /* Check if queue is full */
    uint64_t head = atomic_load_explicit( &(spsc->head), memory_order_acquire );
    int is_full = (int)((head + CHANNEL_N) == producer->tail);

    if( !is_full ) {
        /* Calculate the index in the circular buffer */
        size_t index = producer->tail & (CHANNEL_N - 1);

        /* Calculate the base address of the buffer (assume it follows the SPSC struct in memory) */
        void *buffer_base = (char *)spsc + sizeof(QUE_(spsc_t)) - 112;
        size_t misalignment = (size_t)(buffer_base) % __alignof__(CHANNEL_T);
        size_t adjustment = (__alignof__(CHANNEL_T) - misalignment) & -(misalignment != 0);
        buffer_base += adjustment;

        /* Calculate the address in the buffer by offsetting the index by value_size */
        void *addr = (char *)buffer_base + (index * sizeof(CHANNEL_T));

        /* Write the value to the buffer using memcpy */
        memcpy( addr, value, sizeof(CHANNEL_T) );

        /* Increment tail and written counter */
        producer->tail++;
        producer->written++;
    }

    return is_full;
}

static inline void
sync_producer( PRODUCER_(producer_t) *producer ) {
    QUE_(spsc_t) *spsc = producer->spsc;
    producer->written = 0;
    atomic_store_explicit( &(spsc->tail), producer->tail, memory_order_release );
}

static inline int 
consumer_heartbeat( PRODUCER_(producer_t) *producer ) {
    QUE_(spsc_t) *spsc = producer->spsc;

    /* Load the current heartbeat of the consumer */
    size_t heartbeat = atomic_load_explicit( &(spsc->consumer_heartbeat), memory_order_acquire );

    /* Check if the consumer's heartbeat has changed */
    if( heartbeat != producer->last_consumer_heartbeat ) {
        /* Update the last known consumer heartbeat in the producer */
        producer->last_consumer_heartbeat = heartbeat;
        return 1; /* Consumer is active */
    }

    return 0; /* Consumer has not updated its heartbeat */
}

static inline void
PRODUCER_(beat)( PRODUCER_(producer_t) *producer ) {
    QUE_(spsc_t) *spsc = producer->spsc;

    // Atomically increment the producer's heartbeat
    atomic_fetch_add_explicit(&(spsc->producer_heartbeat), 1, memory_order_release);
}