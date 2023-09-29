#include <linux/fs.h>

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
