#include <linux/fs.h>

void *rust_helper_alloc_inode_sb(struct super_block *sb,
				 struct kmem_cache *cache, gfp_t gfp)
{
	return alloc_inode_sb(sb, cache, gfp);
}

struct file *rust_helper_get_file(struct file *f)
{
	return get_file(f);
}

void rust_helper_i_uid_write(struct inode *inode, uid_t uid)
{
	i_uid_write(inode, uid);
}

void rust_helper_i_gid_write(struct inode *inode, gid_t gid)
{
	i_gid_write(inode, gid);
}

struct dentry *rust_helper_dget(struct dentry *dentry)
{
	return dget(dentry);
}

loff_t rust_helper_i_size_read(const struct inode *inode)
{
	return i_size_read(inode);
}

void rust_helper_i_size_write(struct inode *inode, loff_t i_size)
{
	return i_size_write(inode, i_size);
}

void rust_helper_inode_lock_shared(struct inode *inode)
{
	inode_lock_shared(inode);
}

void rust_helper_inode_unlock_shared(struct inode *inode)
{
	inode_unlock_shared(inode);
}
