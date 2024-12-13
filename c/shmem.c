#include "shmem.h"

/* Align size to the nearest page size */
size_t
align_to_page_size( size_t size, page_size_t page_size ) {
    size_t page_size_ = page_size;
    if( page_size == STANDARD_PAGE ) {
        page_size_ = sysconf(_SC_PAGE_SIZE);
        if( page_size == -1 ) {
            perror( "failed to retrieve page size" );
            exit(1);
        }
        fprintf( stderr, "DEBUG: page_size_ updated from %u to %zu\n", page_size, page_size_ );
    }
    return (size + page_size_ - 1) & ~(page_size_ - 1);
}

/* Function to open or create shared memory, with support for huge/gigantic pages.
   Remember to check if fd = -1 or shmem = NULL! */
shmem_t
open_or_create_shmem( const char *id, size_t size, page_size_t page_size ) {
    shmem_t shmem = {.fd=1, .mem=NULL};

    int flags = O_RDWR | O_CREAT;
    int mmap_flags = MAP_SHARED;
    const char *huge_hugetlbfs_path = "/mnt/hugepages";
    const char *giga_hugetlbfs_path = "/mnt/gigantic";

    /* Align size to the nearest page size */
    size = align_to_page_size( size, page_size );

    /* Use hugetlbfs for huge and gigantic pages */
#if __linux__
    if( page_size == HUGE_PAGE_2MB || page_size == GIGANTIC_PAGE_1GB )
    {
        mmap_flags |= MAP_HUGETLB; // Enable huge pages

        const char* path = huge_hugetlbfs_path;
        if( page_size == GIGANTIC_PAGE_1GB )
        {
            mmap_flags |= MAP_HUGE_1GB;
            path = giga_hugetlbfs_path;
        }
 
        /* Open a file in the hugetlbfs directory */
        char full_path[256];
        snprintf( full_path, sizeof(full_path), "%s/%s", path, id );
        shmem.fd = open( full_path, O_CREAT | O_RDWR, S_IRUSR | S_IWUSR );
    } else
#endif
    {
        /* For standard pages, just use regular open */
        shmem.fd = shm_open( id, flags, S_IRUSR | S_IWUSR );
    }

    /* Handle fd error */
    if( shmem.fd == -1 )
    {
        perror( "shm_open failed" );
        return shmem;
    }

    /* Resize the shared memory segment, if size differs */
    struct stat stat_buf;
    if( fstat(shmem.fd, &stat_buf) == -1 ){
        perror( "fstat" );
        close( shmem.fd );
        shmem.fd = -1;
        return shmem;
    }
    if( stat_buf.st_size != size ) {
        fprintf( stderr, "st size %ld vs size %ld", stat_buf.st_size, size );
        if( ftruncate( shmem.fd, size ) == -1 ) {
            perror( "ftruncate failed" );
            close( shmem.fd );
            shmem.fd = -1;
            return shmem;
        }
    }

    /* Map the shared memory segment */
    shmem.mem = mmap( NULL, size, PROT_READ | PROT_WRITE, mmap_flags, shmem.fd, 0 );
    if( shmem.mem == MAP_FAILED ) {
        perror( "mmap failed" );
        close( shmem.fd );
        shmem.fd = -1;
        shmem.mem = NULL;
    }

    return shmem;
}

void
close_shmem( shmem_t shmem ) {
    if( shmem.fd != -1) {
        close( shmem.fd );
    }

    if( !shmem.mem) {
        munmap( shmem.mem, shmem.size );
    }
}
