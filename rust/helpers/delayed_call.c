// SPDX-License-Identifier: GPL-2.0

#include <linux/delayed_call.h>

void rust_helper_set_delayed_call(struct delayed_call *call,
				  void (*fn)(void *), void *arg)
{
	set_delayed_call(call, fn, arg);
}
