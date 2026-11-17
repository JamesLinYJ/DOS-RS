#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <sys/statvfs.h>
#include <dirent.h>
#include <fcntl.h>
#include <unistd.h>
#include <string.h>
extern long sys_mountinfo(long, long, long);
#define CHECK(test) do { if (!(test)) { fprintf(stderr, "mount test line %d: %s (errno %d)\n", __LINE__, #test, errno); return __LINE__; } } while (0)

int main(void)
{
    char data[96], shortbuf[2] = {42,42};
    int n, count=0;
    CHECK(sys_mountinfo(0,(long)shortbuf,sizeof shortbuf)==-ERANGE && shortbuf[0]==42);
    CHECK(sys_mountinfo(0,1,sizeof data)==-EFAULT);
    for (int i=0;(n=sys_mountinfo(i,(long)data,sizeof data))>0;++i) {
        const char *p=data,*end=data+n;
        for(int field=0;field<4;++field) {
            const char *nul=memchr(p,0,end-p);
            CHECK(nul && nul!=p);
            printf("%s%s",p,field==3?"\n":" ");
            p=nul+1;
        }
        CHECK(p==end);
        ++count;
    }
    CHECK(count==3 && n==0);
    for (int i=0;i<4;++i) {
        const char *name=(const char *[]){"/dos","/dos/c","/dos/c/","/dos/c/df.elf"}[i];
        struct stat st;
        char path[4096];
        CHECK(lstat(name,&st)==0);
        CHECK(readlink(name,path,sizeof path)==-1 && errno==EINVAL);
        CHECK(realpath(name,path)!=NULL);
        CHECK(!strcmp(path,i==2?"/dos/c":name));
    }
    DIR *root=opendir("/");
    CHECK(root);
    int hubs=0;
    struct dirent *root_entry;
    while ((root_entry=readdir(root))) hubs+=!strcmp(root_entry->d_name,"dos");
    CHECK(hubs==1 && closedir(root)==0);
    int old=open("/",O_RDONLY|O_DIRECTORY);
    CHECK(old>=0 && chdir("/dos")==0);
    char cwd[64];
    CHECK(getcwd(cwd,sizeof cwd) && !strcmp(cwd,"/dos"));
    DIR *shares=opendir(".");
    CHECK(shares);
    unsigned found=0;
    struct dirent *de;
    while ((de=readdir(shares))) {
        if (!strcmp(de->d_name,"a")) found|=1;
        if (!strcmp(de->d_name,"b")) found|=2;
        if (!strcmp(de->d_name,"c")) found|=4;
    }
    CHECK(found==7 && !closedir(shares));
    int child=open("c",O_RDONLY|O_DIRECTORY);
    CHECK(child>=0 && fchdir(child)==0 && getcwd(cwd,sizeof cwd) && !strcmp(cwd,"/dos/c"));
    int self=openat(child,"DF.ELF",O_RDONLY);
    CHECK(self>=0 && close(self)==0 && close(child)==0 && fchdir(old)==0 && close(old)==0);
    for (int i=0;i<2;++i) {
        const char *path=i?"/dos/c":"/";
        struct statvfs info;
        CHECK(statvfs(path,&info)==0);
        printf("STATVFS %s %llu %llu %llu %llu\n",i?"fat":"btrfs",(unsigned long long)info.f_frsize,
            (unsigned long long)info.f_blocks,(unsigned long long)info.f_bfree,(unsigned long long)info.f_bavail);
    }
    puts("MOUNT_ABI_OK");
    return 0;
}
