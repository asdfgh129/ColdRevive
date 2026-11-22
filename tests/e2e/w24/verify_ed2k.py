# RFC 1320 MD4, independent pure-Python implementation for cross-check.
import struct, json, sys

def _F(x,y,z): return (x & y) | (~x & z)
def _G(x,y,z): return (x & y) | (x & z) | (y & z)
def _H(x,y,z): return x ^ y ^ z
def _rol(v,n):
    v &= 0xffffffff
    return ((v << n) | (v >> (32-n))) & 0xffffffff

def md4(data):
    ml = len(data) * 8
    data = bytearray(data) + b'\x80'
    while len(data) % 64 != 56: data += b'\x00'
    data += struct.pack('<Q', ml & 0xffffffffffffffff)
    a0,b0,c0,d0 = 0x67452301,0xefcdab89,0x98badcfe,0x10325476
    for i in range(0,len(data),64):
        X = list(struct.unpack('<16I', data[i:i+64]))
        a,b,c,d = a0,b0,c0,d0
        # Round 1
        for j in range(16):
            k = j; s = [3,7,11,19][j%4]
            if j%4==0: a=_rol(a+_F(b,c,d)+X[k],s)
            elif j%4==1: d=_rol(d+_F(a,b,c)+X[k],s)
            elif j%4==2: c=_rol(c+_F(d,a,b)+X[k],s)
            else: b=_rol(b+_F(c,d,a)+X[k],s)
        # Round 2
        order=[0,4,8,12,1,5,9,13,2,6,10,14,3,7,11,15]
        for j in range(16):
            k=order[j]; s=[3,5,9,13][j%4]
            if j%4==0: a=_rol(a+_G(b,c,d)+X[k]+0x5a827999,s)
            elif j%4==1: d=_rol(d+_G(a,b,c)+X[k]+0x5a827999,s)
            elif j%4==2: c=_rol(c+_G(d,a,b)+X[k]+0x5a827999,s)
            else: b=_rol(b+_G(c,d,a)+X[k]+0x5a827999,s)
        # Round 3
        order3=[0,8,4,12,2,10,6,14,1,9,5,13,3,11,7,15]
        for j in range(16):
            k=order3[j]; s=[3,9,11,15][j%4]
            if j%4==0: a=_rol(a+_H(b,c,d)+X[k]+0x6ed9eba1,s)
            elif j%4==1: d=_rol(d+_H(a,b,c)+X[k]+0x6ed9eba1,s)
            elif j%4==2: c=_rol(c+_H(d,a,b)+X[k]+0x6ed9eba1,s)
            else: b=_rol(b+_H(c,d,a)+X[k]+0x6ed9eba1,s)
        a0=(a0+a)&0xffffffff; b0=(b0+b)&0xffffffff
        c0=(c0+c)&0xffffffff; d0=(d0+d)&0xffffffff
    return struct.pack('<4I',a0,b0,c0,d0)

assert md4(b'').hex()=='31d6cfe0d16ae931b73c59d7e0c089c0', md4(b'').hex()
assert md4(b'abc').hex()=='a448017aaf21d8525fc10ae87aa6729d', md4(b'abc').hex()

def ed2k(path):
    data=open(path,'rb').read(); PART=9500*1024
    if len(data)<PART: return md4(data).hex()
    parts=b''
    for i in range(0,len(data),PART): parts+=md4(data[i:i+PART])
    if len(data)%PART==0: parts+=md4(b'')
    return md4(parts).hex()


def main():
    d=json.load(open('tests/e2e/w24/ed2k_links.json'))
    ok=0
    for l in d['links']:
        want=ed2k('tests/e2e/w62/run/'+l['name'])
        got=l['ed2k_link'].split('|')[4]
        print(l['name'],'independent=',want,'daemon=',got,'match=',want==got)
        ok += (want==got)
    print('verified=',ok,'/',len(d['links']))
    return 0 if ok==len(d['links']) else 1


# `md4` / `ed2k` 也被 P0 e2e（tests/e2e/p0/run_p0.py）当独立实现引用：
# 它们只做纯计算，读数据文件的逻辑留在 `main()` 里，import 不产生副作用。
if __name__ == '__main__':
    sys.exit(main())