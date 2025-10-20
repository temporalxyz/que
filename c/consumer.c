#include <stdio.h>
#include <stdint.h>
#include <string.h>
#include <assert.h>
#include <stdatomic.h>
#include <stddef.h>

#include "spsc.c"

#define CONSUMER_CONCAT3(a, b, c) a##b##c
#define CONSUMER_EXPAND_THEN_CONCAT3(a, b, c) CONSUMER_CONCAT3(a, b, c)
#define CONSUMER_(x) CONSUMER_EXPAND_THEN_CONCAT3(CHANNEL_NAME, _consumer_, x)

typedef struct CONSUMER_(consumer) {
    QUE_(spsc_t) * spsc;                  /* Pointer to shared SPSC */
    uint64_t head;                        /* Local head */
    uint64_t items_since_last_sync;       /* Items consumed since last sync */
    uint64_t consumer_index;              /* Consumer index (for future SPMC) */
    uint64_t last_producer_heartbeat;     /* Last known producer heartbeat */
} CONSUMER_(consumer_t);

static inline int
CONSUMER_(join)( void *shmem_region, CONSUMER_(consumer_t) *consumer ) {

    /* Ensure alignment */
    assert( ((size_t)shmem_region % ALIGNMENT) == 0 ); 

    QUE_(spsc_t) *spsc = (QUE_(spsc_t) *)shmem_region;

    /* Join existing queue */
    if( spsc->magic == MAGIC ) {
        if( spsc->capacity != CHANNEL_N ) {
            fprintf( stderr, "Incorrect capacity\n" );
            return 1;
        }

        consumer->spsc = spsc;
        
        /* Set local head to current tail (assume empty upon joining) */
        uint64_t new_head = atomic_load_explicit( &(spsc->tail.value), memory_order_acquire );
        consumer->head = new_head;
        
        /* Update shared head */
        atomic_store_explicit( &(spsc->head.value), new_head, memory_order_release );
        
        consumer->items_since_last_sync = 0;
        consumer->consumer_index = 0;
        consumer->last_producer_heartbeat = atomic_load_explicit( &(spsc->producer_heartbeat.value),
                                                                  memory_order_acquire );
        return 0;
    }
    
    /* Uninitialized */
    if( spsc->magic == 0 ) {
        fprintf( stderr, "Uninitialized channel\n" );
        return 1;
    }

    fprintf( stderr, "Corruption detected\n" );
    return 1;
}

/* Helper function to sync head with shared memory */
static inline void
CONSUMER_(maybe_sync)( CONSUMER_(consumer_t) *consumer ) {
    uint64_t do_sync = consumer->items_since_last_sync >= burst_amount(CHANNEL_N);
    
    if( do_sync ) {
        consumer->items_since_last_sync = 0;
        atomic_store_explicit( &(consumer->spsc->head.value), consumer->head, memory_order_release );
    }
}

static inline int
CONSUMER_(pop)( CONSUMER_(consumer_t) *consumer, CHANNEL_T *value ) {
    QUE_(spsc_t) *spsc = consumer->spsc;

    uint64_t tail = atomic_load_explicit( &(spsc->tail.value), memory_order_acquire );
    int previously_read_or_uninitialized = tail <= consumer->head;
    
    /* Nothing else to read */
    if( previously_read_or_uninitialized ) {
        return 1;
    }
    
    /* Calculate the index in the circular buffer */
    uint64_t index = consumer->head & (CHANNEL_N - 1);
    
    /* Get buffer pointer (starts at offset 640) */
    void *buffer_base = spsc + 1;
    void *addr = (char *)buffer_base + (index * sizeof(CHANNEL_T));
    
    /* Read value */
    memcpy( value, addr, sizeof(CHANNEL_T) );
    
    /* Success - increment head and sync if needed */
    consumer->head++;
    consumer->items_since_last_sync++;
    CONSUMER_(maybe_sync)(consumer);
    
    return 0;
}

static inline void
CONSUMER_(beat)( CONSUMER_(consumer_t) *consumer ) {
    QUE_(spsc_t) *spsc = consumer->spsc;

    /* Atomically increment the consumer's heartbeat */
    atomic_fetch_add_explicit(&(spsc->consumer_heartbeat.value), 1, memory_order_release);
}

static inline int
CONSUMER_(producer_heartbeat)( CONSUMER_(consumer_t) *consumer ) {
    QUE_(spsc_t) *spsc = consumer->spsc;

    /* Load the current heartbeat of the producer */
    uint64_t heartbeat = atomic_load_explicit( &(spsc->producer_heartbeat.value), memory_order_acquire );

    /* Check if the producer's heartbeat has changed */
    if( heartbeat != consumer->last_producer_heartbeat ) {
        /* Update the last known producer heartbeat */
        consumer->last_producer_heartbeat = heartbeat;
        return 1; /* Producer is active */
    }

    return 0; /* Producer has not updated its heartbeat */
}

/* Get pointer to padding area (can be used for metadata) */
static inline void*
CONSUMER_(get_padding_ptr)( CONSUMER_(consumer_t) *consumer ) {
    return consumer->spsc->padding;
}
