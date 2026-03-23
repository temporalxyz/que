#ifndef QUE_COMMON_H
#define QUE_COMMON_H

typedef long page_size_t;

#ifdef __linux__
#define STANDARD_PAGE      1
#define HUGE_PAGE_2MB      (2 * 1024 * 1024)
#define GIGANTIC_PAGE_1GB  (1 * 1024 * 1024 * 1024)
#else
#define STANDARD_PAGE      4096
#endif


#endif /* QUE_COMMON_H */
