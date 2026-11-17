#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <sys/stat.h>
#include <unistd.h>
#define CHECK(n,cond) do {if (!(cond)) { const char text[]="directory fd failure " #n "\n";write(2,text,sizeof text-1);return n;} } while(0)
int main(void) {
    char sample[16]={0};struct stat st, other;
    int root=open("/",O_RDONLY|O_DIRECTORY|O_CLOEXEC);
    CHECK(2,root>=0 && fcntl(root,F_GETFD)==FD_CLOEXEC);
    CHECK(3,mkdirat(root,"atwork",0755)==0);
    int dir=openat(root,"atwork",O_RDONLY|O_DIRECTORY);
    CHECK(4,dir>=0 && fstatat(-7,"/atwork",&st,0)==0 && S_ISDIR(st.st_mode));
    CHECK(5,renameat(root,"atwork",root,"at-moved")==0);
    int fd=openat(dir,"data",O_CREAT|O_EXCL|O_RDWR,0600);
    CHECK(6,fd>=0 && fstatat(fd,"",&st,AT_EMPTY_PATH)==0);
    CHECK(7,write(fd,"abcde",5)==5 && ftruncate(fd,3)==0 && lseek(fd,0,SEEK_CUR)==5);
    CHECK(8,ftruncate(fd,7)==0 && lseek(fd,0,SEEK_SET)==0 && read(fd,sample,7)==7 && sample[2]=='c' && sample[3]==0 && sample[6]==0);
    CHECK(9,ftruncate(fd,-1)<0 && errno==EINVAL && fstat(fd,&st)==0 && st.st_size==7);
    CHECK(10,symlinkat("data",dir,"sym")==0 && readlinkat(dir,"sym",sample,2)==2 && sample[0]=='d' && sample[1]=='a');
    CHECK(11,fstatat(dir,"sym",&st,AT_SYMLINK_NOFOLLOW)==0 && S_ISLNK(st.st_mode));
    CHECK(12,openat(dir,"sym",O_NOFOLLOW|O_RDONLY)<0 && errno==ELOOP);
    CHECK(13,linkat(dir,"sym",root,"at-hard",AT_SYMLINK_FOLLOW)==0 && fstatat(root,"at-hard",&other,0)==0 && fstat(fd,&st)==0 && other.st_ino==st.st_ino);
    CHECK(14,fchmod(fd,0644)==0 && fchown(fd,123,456)==0 && fstat(fd,&st)==0 && (st.st_mode&0777)==0644 && st.st_uid==123);
    CHECK(15,fcntl(fd,F_SETFD,FD_CLOEXEC)==0);
    int copy=fcntl(fd,F_DUPFD,12);
    CHECK(16,copy>=12 && fcntl(copy,F_GETFD)==0 && fcntl(fd,F_GETFD)==FD_CLOEXEC);
    int cl=fcntl(copy,F_DUPFD_CLOEXEC,22);
    CHECK(17,cl>=22 && fcntl(cl,F_GETFD)==FD_CLOEXEC);
    CHECK(18,fcntl(copy,F_SETFL,O_APPEND|O_NONBLOCK)==0 && (fcntl(fd,F_GETFL)&(O_APPEND|O_NONBLOCK))==(O_APPEND|O_NONBLOCK));
    CHECK(19,lseek(fd,0,SEEK_SET)==0 && write(copy,"!",1)==1 && lseek(fd,0,SEEK_CUR)==8);
    CHECK(20,truncate("/at-hard",4)==0 && fsync(fd)==0 && access("/at-hard",F_OK)==0);
    CHECK(21,unlinkat(dir,"sym",0)==0 && unlinkat(dir,"data",0)==0);
    CHECK(22,unlinkat(root,"at-moved",AT_REMOVEDIR)==0 && fstat(fd,&st)==0 && st.st_nlink==1 && st.st_size==4);
    CHECK(23,close(cl)==0 && close(copy)==0 && close(fd)==0 && close(dir)==0 && close(root)==0);
    int pair[2];CHECK(24,pipe(pair)==0 && fcntl(pair[0],F_SETFL,O_NONBLOCK)==0);
    CHECK(25,read(pair[0],sample,1)<0 && errno==EAGAIN && write(pair[1],"!",1)==1 && read(pair[0],sample,1)==1 && sample[0]=='!');
    CHECK(26,close(pair[1])==0 && read(pair[0],sample,1)==0 && close(pair[0])==0);
    return write(1,"directory fd and file flags OK\n",31)!=31;
}
