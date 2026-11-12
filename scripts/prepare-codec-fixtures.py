#!/usr/bin/env python3
"""Synthetic audio only. The optional APE encoder is a QA tool, never bundled."""
import argparse,math,struct,subprocess,wave
from pathlib import Path
parser=argparse.ArgumentParser();parser.add_argument('directory',type=Path);parser.add_argument('--ape-encoder',type=Path,required=True);args=parser.parse_args()
args.directory.mkdir(parents=True,exist_ok=True)
for bits in [16,32]:
    with wave.open(str(args.directory/f'reference{bits}.wav'),'wb') as out:
        out.setnchannels(2);out.setsampwidth(bits//8);out.setframerate(48000)
        frames=[]
        for i in range(48000):
            if bits==16: values=(int(6000*math.sin(2*math.pi*440*i/48000)),int(4000*math.sin(2*math.pi*660*i/48000)))
            else:values=(((i*7919)&0x7fffffff)-0x40000000,0x12345678-(i*3571))
            frames.append(struct.pack('<hh' if bits==16 else '<ii',*values))
        out.writeframes(b''.join(frames))
for extension,codec,reference in [('wv','wavpack','reference32.wav'),('wma','wmav2','reference16.wav'),('opus','libopus','reference16.wav')]:
    subprocess.run(['ffmpeg','-v','error','-y','-i',str(args.directory/reference),'-c:a',codec,'-metadata','title=Synthetic codec fixture',str(args.directory/f'fixture.{extension}')],check=True)
subprocess.run([str(args.ape_encoder.resolve()),str((args.directory/'reference16.wav').resolve()),str((args.directory/'fixture.ape').resolve()),'-c2000'],check=True)
print('Synthetic codec fixtures ready')
