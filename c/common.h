#ifndef QUE_COMMON_H
#define QUE_COMMON_H



/* Enum for page size */
typedef enum {
    /* shm can be truncated and does not need to be rounded up to nearest page size */
    STANDARD_PAGE = 1, 
    HUGE_PAGE_2MB = 2 * 1024 * 1024,
    GIGANTIC_PAGE_1GB = 1 * 1024 * 1024 * 1024
} page_size_t;


#endif /* QUE_COMMON_H */