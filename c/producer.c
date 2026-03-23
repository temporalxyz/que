#include <stdio.h>
#include <stdint.h>
#include <string.h>
#include <assert.h>
#include <stdatomic.h>
#include <stddef.h>
#include <stdbool.h>

#include "spsc.c"

#define PRODUCER_CONCAT3(a, b, c) a##b##c
#define PRODUCER_EXPAND_THEN_CONCAT3(a, b, c) PRODUCER_CONCAT3(a, b, c)
#define PRODUCER_(x) PRODUCER_EXPAND_THEN_CONCAT3(CHANNEL_NAME, _producer_, x)

typedef struct PRODUCER_(producer) {
    QUE_(spsc_t) * spsc;                  /* Pointer to shared SPSC */
    uint64_t tail;                        /* Local tail */
    uint64_t written;                     /* Number of written elements since last sync */
    uint64_t last_consumer_heartbeat;     /* Last known consumer heartbeat */
} PRODUCER_(producer_t);

/* Reservation struct for batch writes */
typedef struct PRODUCER_(reservation) {
    PRODUCER_(producer_t) *producer;
    uint64_t start_tail;
    uint64_t count;
    uint64_t written;
    bool committed;
} PRODUCER_(reservation_t);

static inline int
PRODUCER_(join_or_initialize)( void *shmem_region, PRODUCER_(producer_t)* producer ) {

    /* Capacity must be a power of two */
    assert( CHANNEL_N > 0 && (CHANNEL_N & (CHANNEL_N - 1)) == 0 );  

    /* Ensure alignment */
    assert( ((size_t)shmem_region % ALIGNMENT) == 0 ); 

    QUE_(spsc_t) *spsc = (QUE_(spsc_t) *)shmem_region;

    /* Check magic */
    uint64_t magic = spsc->magic;
    uint64_t capacity = spsc->capacity;

    /* Join existing queue */
    if( magic == MAGIC ) {
        if( capacity != CHANNEL_N ) {
            fprintf( stderr, "Incorrect capacity\n" );
            return 1;
        }

        /* Increment producer heartbeat to signal we've joined */
        atomic_fetch_add_explicit( &(spsc->producer_heartbeat.value), 1, memory_order_release );

        producer->spsc = spsc;
        producer->tail = atomic_load_explicit( &(spsc->tail.value), memory_order_acquire );
        producer->written = 0;
        producer->last_consumer_heartbeat = atomic_load_explicit( &(spsc->consumer_heartbeat.value),
                                                                  memory_order_acquire );
        return 0;
    }
    
    /* Initialization */
    if( magic == 0 ) {
        /* Initialize the SPSC structure */
        atomic_store_explicit( &(spsc->tail.value), 0, memory_order_release );
        atomic_store_explicit( &(spsc->head.value), 0, memory_order_release );
        atomic_store_explicit( &(spsc->producer_heartbeat.value), 0, memory_order_release );
        atomic_store_explicit( &(spsc->consumer_heartbeat.value), 0, memory_order_release );
        spsc->capacity = CHANNEL_N;
        spsc->magic = MAGIC;

        producer->spsc = spsc;
        producer->tail = 0;
        producer->written = 0;
        producer->last_consumer_heartbeat = atomic_load_explicit( &(spsc->consumer_heartbeat.value), 
                                                                  memory_order_acquire );
        return 0;
    }

    fprintf( stderr, "Corruption detected\n" );
    return 1;
}

/* Join existing initialized channel */
static inline int
PRODUCER_(join)( void *shmem_region, PRODUCER_(producer_t)* producer ) {

    /* Capacity must be a power of two */
    assert( CHANNEL_N > 0 && (CHANNEL_N & (CHANNEL_N - 1)) == 0 );  

    /* Ensure alignment */
    assert( ((size_t)shmem_region % ALIGNMENT) == 0 ); 

    QUE_(spsc_t) *spsc = (QUE_(spsc_t) *)shmem_region;

    uint64_t magic = spsc->magic;
    uint64_t capacity = spsc->capacity;
    
    if( magic == MAGIC ) {
        if( capacity != CHANNEL_N ) {
            fprintf( stderr, "Incorrect capacity\n" );
            return 1;
        }

        producer->spsc = spsc;
        producer->tail = atomic_load_explicit( &(spsc->tail.value), memory_order_acquire );
        producer->written = 0;
        producer->last_consumer_heartbeat = atomic_load_explicit( &(spsc->consumer_heartbeat.value),
                                                                  memory_order_acquire );
        return 0;
    }
    
    if( magic == 0 ) {
        fprintf( stderr, "Uninitialized channel\n" );
        return 1;
    }

    fprintf( stderr, "Corruption detected\n" );
    return 1;
}

/* Synchronize the local tail with shared memory */
static inline void
PRODUCER_(sync)( PRODUCER_(producer_t) *producer ) {
    producer->written = 0;
    atomic_store_explicit( &(producer->spsc->tail.value), producer->tail, memory_order_release );
}

/* Push with backpressure - returns 0 on success, 1 if full */
static inline int
PRODUCER_(push)( PRODUCER_(producer_t) *producer, void *value ) {
    QUE_(spsc_t) *spsc = producer->spsc;

    /* Check if queue is full */
    uint64_t head = atomic_load_explicit( &(spsc->head.value), memory_order_acquire );
    int is_full = (producer->tail == head + CHANNEL_N);

    if( is_full ) {
        return 1; /* Queue is full */
    }

    /* Calculate the index in the circular buffer */
    uint64_t index = producer->tail & (CHANNEL_N - 1);

    /* Get buffer pointer (starts at offset 640) */
    void *buffer_base = spsc + 1;
    void *addr = (char *)buffer_base + (index * sizeof(CHANNEL_T));

    /* Write the value to the buffer */
    memcpy( addr, value, sizeof(CHANNEL_T) );

    /* Increment tail and written counter */
    producer->tail++;
    producer->written++;

    // /* Sync if we've written enough */
    // if( producer->written == burst_amount(CHANNEL_N) ) {
    //     PRODUCER_(sync)(producer);
    // }

    return 0; /* Success */
}

static inline void
PRODUCER_(beat)( PRODUCER_(producer_t) *producer ) {
    QUE_(spsc_t) *spsc = producer->spsc;

    /* Atomically increment the producer's heartbeat */
    atomic_fetch_add_explicit(&(spsc->producer_heartbeat.value), 1, memory_order_release);
}

static inline int 
PRODUCER_(consumer_heartbeat)( PRODUCER_(producer_t) *producer ) {
    QUE_(spsc_t) *spsc = producer->spsc;

    /* Load the current heartbeat of the consumer */
    uint64_t heartbeat = atomic_load_explicit( &(spsc->consumer_heartbeat.value), memory_order_acquire );

    /* Check if the consumer's heartbeat has changed */
    if( heartbeat != producer->last_consumer_heartbeat ) {
        /* Update the last known consumer heartbeat */
        producer->last_consumer_heartbeat = heartbeat;
        return 1; /* Consumer is active */
    }

    return 0; /* Consumer has not updated its heartbeat */
}

/* Get pointer to padding area (can be used for metadata) */
static inline void*
PRODUCER_(get_padding_ptr)( PRODUCER_(producer_t) *producer ) {
    return producer->spsc->padding;
}

/* Reserve space for batch writes */
static inline int
PRODUCER_(reserve)( PRODUCER_(producer_t) *producer, uint64_t count, PRODUCER_(reservation_t) *reservation ) {
    if( count == 0 || count > CHANNEL_N ) {
        return 1; /* Invalid size */
    }

    /* Check if we have space */
    uint64_t head = atomic_load_explicit( &(producer->spsc->head.value), memory_order_acquire );
    uint64_t available_space = CHANNEL_N - (producer->tail - head);
    
    if( count > available_space ) {
        return 1; /* Not enough space */
    }

    /* Initialize reservation */
    reservation->producer = producer;
    reservation->start_tail = producer->tail;
    reservation->count = count;
    reservation->written = 0;
    reservation->committed = false;

    return 0; /* Success */
}

/* Write next value in a reservation */
static inline CHANNEL_T *
PRODUCER_(reservation_get_next)( PRODUCER_(reservation_t) *reservation ) {
    // assert( reservation->written < reservation->count );

    uint64_t index = (reservation->start_tail + reservation->written) & (CHANNEL_N - 1);
    
    /* Get buffer pointer */
    void *buffer_base = reservation->producer->spsc + 1;
    void *addr = (char *)buffer_base + (index * sizeof(CHANNEL_T));

    return addr;
}

/* Write next value in a reservation */
static inline void
PRODUCER_(reservation_write_next)( PRODUCER_(reservation_t) *reservation ) {
    reservation->written++;
}

/* Write all values from array to reservation */
static inline void
PRODUCER_(reservation_write_all)( PRODUCER_(reservation_t) *reservation, void *values, uint64_t count ) {
    // assert( reservation->written + count <= reservation->count );

    if( count == 0 ) {
        return;
    }

    uint64_t start_index = (reservation->start_tail + reservation->written) & (CHANNEL_N - 1);
    uint64_t end_index = start_index + count;
    
    void *buffer_base = reservation->producer->spsc + 1;

    if( end_index <= CHANNEL_N ) {
        /* No wraparound - single copy */
        memcpy( (char *)buffer_base + (start_index * sizeof(CHANNEL_T)),
                values,
                count * sizeof(CHANNEL_T) );
    } else {
        /* Wraparound - two copies */
        uint64_t first_part_len = CHANNEL_N - start_index;
        
        /* Copy first part (until end of buffer) */
        memcpy( (char *)buffer_base + (start_index * sizeof(CHANNEL_T)),
                values,
                first_part_len * sizeof(CHANNEL_T) );
        
        /* Copy second part (from beginning of buffer) */
        memcpy( buffer_base,
                (char *)values + (first_part_len * sizeof(CHANNEL_T)),
                (count - first_part_len) * sizeof(CHANNEL_T) );
    }

    reservation->written += count;
}

/* Commit a reservation */
static inline void
PRODUCER_(reservation_commit)( PRODUCER_(reservation_t) *reservation ) {
    reservation->committed = true;

    /* Only advance tail by amount actually written */
    reservation->producer->tail += reservation->written;
    reservation->producer->written += reservation->written;

    /* Sync  */
    PRODUCER_(sync)(reservation->producer);
}

/* Cancel a reservation without publishing */
static inline void
PRODUCER_(reservation_cancel)( PRODUCER_(reservation_t) *reservation ) {
    /* Nothing to do - just don't advance tail */
    reservation->committed = true; /* Mark as handled */
}
