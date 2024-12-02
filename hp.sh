#!/bin/bash

if [ "$#" -ne 1 ]; then
    echo "Usage: $0 <num_pages>"
    exit 1
fi

NUM_PAGES="$1"

echo "Allocating ${NUM_PAGES} 2MiB hugepages"

# Enable 1GiB pages
if [ ! -e /sys/kernel/mm/hugepages/hugepages-2048kB/nr_hugepages ]; then
    echo "The system does not support 2MiB hugepages"
    exit 1
fi

# Allocate hugepages
echo $NUM_PAGES | sudo tee /sys/kernel/mm/hugepages/hugepages-2048kB/nr_hugepages

# Verify allocation
ACTUAL_NUM_PAGES=$(cat /sys/kernel/mm/hugepages/hugepages-2048kB/nr_hugepages)

if [ "$ACTUAL_NUM_PAGES" -ne "$NUM_PAGES" ]; then
    echo "Failed to allocate hugepages"
    exit 1
fi

echo "Successfully allocated ${NUM_PAGES} 2MiB hugepages"