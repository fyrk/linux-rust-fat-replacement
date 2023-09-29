// SPDX-License-Identifier: GPL-2.0

#include <linux/blk-mq.h>
#include <linux/blkdev.h>

sector_t rust_helper_bdev_nr_sectors(struct block_device *bdev)
{
	return bdev_nr_sectors(bdev);
}

void *rust_helper_blk_mq_rq_to_pdu(struct request *rq)
{
	return blk_mq_rq_to_pdu(rq);
}

struct request *rust_helper_blk_mq_rq_from_pdu(void *pdu)
{
	return blk_mq_rq_from_pdu(pdu);
}
