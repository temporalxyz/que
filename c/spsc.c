#ifndef QUE_QUE_H
#define QUE_QUE_H

#include <stdatomic.h>
#include <stddef.h>
#include <stdint.h>

#ifndef CHANNEL_NAME
#error "CHANNEL_NAME must be defined"
#endif

#ifndef CHANNEL_T
#error "CHANNEL_T must be defined"
#endif

#ifndef CHANNEL_N
#error "CHANNEL_N must be defined"
#endif

#if ((CHANNEL_N) < 1UL) || ((CHANNEL_N & (CHANNEL_N - 1)) != 0)
#error "CHANNEL_N must be positive/nonzero and a power of 2"
#endif

#define QUE_CONCAT3(a, b, c) a##b##c
#define QUE_EXPAND_THEN_CONCAT3(a, b, c) QUE_CONCAT3(a, b, c)
#define QUE_(x) QUE_EXPAND_THEN_CONCAT3(CHANNEL_NAME, _, x)

/* Constants */
#define MAGIC 5494763520971851092 /* "TEMPORAL" */
#define ALIGNMENT 128

typedef struct {
    atomic_size_t value;
    char padding[128 - sizeof(atomic_size_t)];
} __attribute__((aligned(128))) cache_padded_atomic_t;

/* Channel struct matching Rust layout exactly */
typedef struct QUE_(spsc) {
    /* Offset 0: tail (128 bytes) */
    cache_padded_atomic_t tail;
    
    /* Offset 128: head (128 bytes) */
    cache_padded_atomic_t head;
    
    /* Offset 256: producer_heartbeat (128 bytes) */
    cache_padded_atomic_t producer_heartbeat;
    
    /* Offset 384: consumer_heartbeat (128 bytes) */
    cache_padded_atomic_t consumer_heartbeat;
    
    /* Offset 512: padding (112 bytes) */
    char padding[112];
    
    /* Offset 624: capacity (8 bytes) */
    uint64_t capacity;
    
    /* Offset 632: magic (8 bytes) */
    uint64_t magic;
    
    /* Offset 640: buffer starts here */
    /* Buffer follows immediately after in memory */
} __attribute__((aligned(128))) QUE_(spsc_t);

/* Calculate burst amount - 1/4 of buffer or minimum 1 */
static inline uint64_t
burst_amount( uint64_t N ) {
    uint64_t x = N / 4;
    return (x == 0) ? 1 : x;
}

/* Helper to get buffer pointer */
static inline void*
get_buffer_ptr( QUE_(spsc_t) *spsc ) {
    return (char *)spsc + 640;  /* Buffer starts at offset 640 */
}

#endif /* QUE_QUE_H */
