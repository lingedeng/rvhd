#rvhd
rvhd-util-convert

使用rust实现vhd-util-convert功能
代码参考：[rdisk](https://github.com/vsrs/rdisk)

## vhd-util-convert
Re-packaged vhd-util from [xen-4.4.0](http://bits.xensource.com/oss-xen/release/4.4.0/xen-4.4.0.tar.gz) with patch to add the convert command applied.

Original patch from Alfred Song: http://old-list-archives.xen.org/archives/html/xen-devel/2010-07/msg00694.html

```
apt-get install -y uuid-dev build-essential
make

```

Ref:
* http://blogs.citrix.com/2012/10/04/convert-a-raw-image-to-xenserver-vhd/
* https://developer.rackspace.com/blog/bootstrap-your-qcow-images-for-the-rackspace-public-cloud/
