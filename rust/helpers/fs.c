#include <linux/fs.h>

struct file *rust_helper_get_file(struct file *f)
{
	return get_file(f);
}
