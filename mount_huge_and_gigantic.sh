sudo mkdir /mnt/hugepages
sudo mount -t hugetlbfs none /mnt/hugepages
sudo mount -t hugetlbfs -o pagesize=2M none /mnt/hugepages

sudo mkdir /mnt/gigantic
sudo mount -t hugetlbfs none /mnt/gigantic
sudo mount -t hugetlbfs -o pagesize=1G none /mnt/gigantic
