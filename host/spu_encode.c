/* Host-only PSX ADPCM encoder. Exhaustive per-block filter/shift selection.
 * No retail code/data and no runtime dependency. Input/output are little-endian.
 * Each first block uses filter0 so a loop/restarted stream has defined history.
 * Payload flags are zero; the eventual resident/streaming transport owns flags.
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <math.h>
static const int coef[5][2]={{0,0},{60,0},{115,-52},{98,-55},{122,-60}};
static int clamp(int n,int lo,int hi){return n<lo?lo:n>hi?hi:n;}
static int prediction(int a,int b,int f){return (a*coef[f][0]+b*coef[f][1]+32)>>6;}
int main(int argc,char **argv){
 if(argc!=3){fprintf(stderr,"usage: spu_encode input.s16le output.adpcm\n");return 2;}
 FILE *in=fopen(argv[1],"rb"),*out=fopen(argv[2],"wb");if(!in||!out)return 3;
 int hist1=0,hist2=0;long long count=0;double signal=0,error=0;int first=1;
 for(;;){
  unsigned char bytes[56];size_t n=fread(bytes,1,56,in);if(!n)break;if(n&1)return 4;
  int src[28]={0};for(size_t i=0;i<n/2;i++)src[i]=(int16_t)((unsigned)bytes[2*i]|((unsigned)bytes[2*i+1]<<8));
  double best=1e100;unsigned char packed[16]={0};int best1=0,best2=0;
  for(int f=0;f<(first?1:5);f++)for(int shift=0;shift<=12;shift++){
   int a=hist1,b=hist2,unit=1<<(12-shift);double cost=0;unsigned char trial[16]={0};trial[0]=(unsigned char)((f<<4)|shift);
   for(int i=0;i<28;i++){
    int pred=prediction(a,b,f),q=clamp((int)lround((double)(src[i]-pred)/unit),-8,7);
    int decoded=clamp(pred+q*unit,-32768,32767);double d=src[i]-decoded;cost+=d*d;
    trial[2+i/2]|=(unsigned char)((q&15)<<((i&1)*4));b=a;a=decoded;
   }
   if(cost<best){best=cost;best1=a;best2=b;for(int i=0;i<16;i++)packed[i]=trial[i];}
  }
  if(fwrite(packed,1,16,out)!=16)return 5;
  /* Score only actual samples; final zero padding is reported separately. */
  int a=hist1,b=hist2,f=packed[0]>>4,unit=1<<(12-(packed[0]&15));
  for(int i=0;i<28;i++){
   int q=(packed[2+i/2]>>((i&1)*4))&15;if(q>=8)q-=16;
   int v=clamp(prediction(a,b,f)+q*unit,-32768,32767);b=a;a=v;
   if(i<(int)n/2){double d=src[i]-v;error+=d*d;signal+=(double)src[i]*src[i];count++;}
  }
  hist1=best1;hist2=best2;first=0;
 }
 fclose(in);fclose(out);
 printf("{\"samples\":%lld,\"signal_energy\":%.17g,\"error_energy\":%.17g,\"snr_db\":%.8g}\n",count,signal,error,error>0&&signal>0?10*log10(signal/error):999.0);
 return 0;
}
