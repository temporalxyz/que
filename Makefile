CC := gcc
CFLAGS := -O3 -march=native

CONSUMER_SRCS := c/shmem.c c/util.c c/test_consumer.c
PRODUCER_SRCS := c/shmem.c c/util.c c/test_producer.c
LAYOUT_SRCS := c/shmem.c c/util.c c/layout.c

all: consumer.out producer.out layout.out

consumer.out: $(CONSUMER_SRCS)
	$(CC) $(CFLAGS) $^ -o $@

producer.out: $(PRODUCER_SRCS)
	$(CC) $(CFLAGS) $^ -o $@

layout.out: $(LAYOUT_SRCS)
	$(CC) $(CFLAGS) $^ -o $@