#ifndef QUE_SHMEM_H
#define QUE_SHMEM_H

#include <stdio.h>
#include <stdlib.h>
#include <stddef.h>
#include <string.h>
#include <stdatomic.h>
#include <stdint.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
#include <assert.h>

#if __linux__
#include <linux/mman.h>
#endif

#include "common.h"

typedef struct {
    void *mem;     /* Pointer to shared memory */
    int fd;        /* File descriptor */
    uint64_t size; /* Size of mem */
} shmem_t;

/* Align size to the nearest page size */
size_t
align_to_page_size( size_t size, page_size_t page_size );

/* Function to open or create shared memory, with support for huge/gigantic pages */
shmem_t
open_or_create_shmem( const char *id, size_t size, page_size_t page_size );

/* Function to closed shared memory */
void
close_shmem( shmem_t shmem );

#endif /* QUE_SHMEM_H */