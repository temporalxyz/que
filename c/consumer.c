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
    QUE_(spsc_t) * spsc;         /* Pointer to shared SPSC */
    uint64_t head;                    /* Local head */
    uint64_t last_producer_heartbeat; /* Last known producer heartbeat */
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
        /* set local head to current tail */
        consumer->head = atomic_load_explicit( &(spsc->tail), memory_order_acquire );
        consumer->last_producer_heartbeat = atomic_load_explicit( &(spsc->producer_heartbeat),
                                                                  memory_order_acquire );
        return 0;
    }
    
    /* Unintialized */
    if( spsc->magic == 0 ) {
        fprintf( stderr, "Uninitialized channel\n" );
        return 1;
    }

    fprintf( stderr, "Corruption detected\n" );
    return 1;
}

static inline int
CONSUMER_(pop)( CONSUMER_(consumer_t) *consumer, CHANNEL_T *value ) {


    QUE_(spsc_t) *spsc = consumer->spsc;

    for(;;) {
        uint64_t index = consumer->head & (CHANNEL_N - 1);

        /* Calculate the base address of the buffer (assume it follows the SPSC struct in memory) */
        void *buffer_base = (char *)spsc + sizeof(QUE_(spsc_t)) - 112;
        size_t misalignment = (size_t)(buffer_base) % __alignof__(CHANNEL_T);
        size_t adjustment = (__alignof__(CHANNEL_T) - misalignment) & -(misalignment != 0);
        buffer_base += adjustment;

        /* Calculate the address in the buffer by offsetting the index by value_size */
        void *addr = (char *)buffer_base + (index * sizeof(CHANNEL_T));

        /* Optimistically read value */
        memcpy( value, addr, sizeof(CHANNEL_T) );

        /* Check if valid
           1) is not overrun
           2) is not previously read value */
        uint64_t tail = atomic_load_explicit( &(spsc->tail), memory_order_acquire );
        int not_overrun = tail <= (consumer->head + (CHANNEL_N - burst_amount(CHANNEL_N)));
        int previously_read_or_uninitialized = tail <= consumer->head;

        if( previously_read_or_uninitialized ) return 1;

        if( !not_overrun ) {
            /* Reset to next integer */
            consumer->head = tail - (CHANNEL_N - burst_amount(CHANNEL_N));
            continue;
        }

        /* Success */
        consumer->head++;
        return 0;
    }

    /* unreachable. TODO: bound loop */
    fprintf( stderr, "unreachable" );
    return 1;
}

static inline int
CONSUMER_(lossless_pop)( CONSUMER_(consumer_t) *consumer, CHANNEL_T *value ) {


    QUE_(spsc_t) *spsc = consumer->spsc;

    for(;;) {
        uint64_t index = consumer->head & (CHANNEL_N - 1);

        /* Calculate the base address of the buffer (assume it follows the SPSC struct in memory) */
        void *buffer_base = (char *)spsc + sizeof(QUE_(spsc_t)) - 112;
        size_t misalignment = (size_t)(buffer_base) % __alignof__(CHANNEL_T);
        size_t adjustment = (__alignof__(CHANNEL_T) - misalignment) & -(misalignment != 0);
        buffer_base += adjustment;

        /* Calculate the address in the buffer by offsetting the index by value_size */
        void *addr = (char *)buffer_base + (index * sizeof(CHANNEL_T));

        /* Optimistically read value */
        memcpy( value, addr, sizeof(CHANNEL_T) );

        /* Check if valid
           1) is not previously read value */
        uint64_t tail = atomic_load_explicit( &(spsc->tail), memory_order_acquire );
        int previously_read_or_uninitialized = tail <= consumer->head;

        if( previously_read_or_uninitialized ) return 1;

        /* Success */
        consumer->head++;
        return 0;
    }

    /* unreachable. TODO: bound loop */
    return 1;
}

static inline void
CONSUMER_(beat)( CONSUMER_(consumer_t) *consumer) {
    QUE_(spsc_t) *spsc = consumer->spsc;

    // Atomically increment the consumer's heartbeat
    atomic_fetch_add_explicit(&(spsc->consumer_heartbeat), 1, memory_order_release);
}