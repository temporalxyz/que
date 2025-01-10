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

typedef struct QUE_(spsc) {
    atomic_size_t tail __attribute__((aligned(128)));
    atomic_size_t head __attribute__((aligned(128)));
    atomic_size_t producer_heartbeat __attribute__((aligned(128)));
    atomic_size_t consumer_heartbeat __attribute__((aligned(128)));
    char c_padding[128 - sizeof(size_t)]; // Add padding; this one needs to be declared explicitly in C.
    char padding[128 - 2 * sizeof(uint64_t)]; // Add padding to make size a multiple of 128 bytes. This is the padding seen in the Rust lib.
    uint64_t capacity;
    uint64_t magic; /* Magic value to check initialization */
} __attribute__((aligned(128))) QUE_(spsc_t);

static inline uint64_t
burst_amount( uint64_t N ) {
    uint64_t x = N / 4;
    return (x == 0) ? 1 : x;
}

#endif /* QUE_QUE_H */