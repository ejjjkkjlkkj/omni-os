"""STN1 worker: persistent CPU model, no downloads, bounded binary PCM frames.

Input: one UTF-8 JSON request per line. Output: <4sIII> magic, kind, rate, bytes.
Kinds: PCM=0, DONE=1, ERROR=2, READY=3. PCM is little-endian f32 mono 48 kHz.
"""
import argparse, hashlib, json, struct, sys
from pathlib import Path

def frame(kind, data=b''):
    sys.stdout.buffer.write(struct.pack('<4sIII', b'STN1', kind, 48000, len(data)))
    sys.stdout.buffer.write(data)
    sys.stdout.buffer.flush()

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--home',type=Path,required=True)
    home=parser.parse_args().home
    try:
        import numpy as np
        import onnxruntime as rt
        from scipy.signal import resample_poly
        from kokoro_onnx import Kokoro
        manifest=json.loads((home/'models.json').read_text(encoding='utf-8-sig'))
        for item in manifest['files']:
            p=home/'models'/item['name']
            with p.open('rb') as f:
                if hashlib.file_digest(f,'sha256').hexdigest()!=item['sha256']:
                    raise ValueError('Model checksum mismatch: '+item['name'])
        options=rt.SessionOptions()
        options.intra_op_num_threads=2
        options.inter_op_num_threads=1
        model=Kokoro.from_session(rt.InferenceSession(str(home/'models/kokoro-v1.0.onnx'),sess_options=options,providers=['CPUExecutionProvider']),str(home/'models/voices-v1.0.bin'))
        frame(3)
    except Exception as e:
        frame(2,str(e).encode('utf-8'));return 1
    for line in sys.stdin.buffer:
        try:
            request=json.loads(line)
            text=request['text']
            if not isinstance(text,str) or len(text.encode('utf-8'))>65536:
                raise ValueError('Invalid text size')
            # Preserve model-level punctuation/context; library chunks without truncation.
            import asyncio
            async def generate():
                async for samples,rate in model.create_stream(text,request['voice'],speed=float(request['speed']),lang=request['lang']):
                    x=np.asarray(samples,dtype=np.float64)
                    if rate!=24000 or not len(x) or not np.isfinite(x).all():
                        raise ValueError('Invalid native model audio')
                    # DC removal and anti-imaging filter before 24->48 kHz conversion.
                    x=x-x.mean()
                    y=resample_poly(x,2,1,window=('kaiser',8.6))
                    peak=float(np.max(np.abs(y)))
                    if not np.isfinite(peak) or peak==0:
                        raise ValueError('Silent/invalid model output')
                    y*=min(1.0,0.89/peak)  # attenuation only; do not amplify quiet phrases
                    fade=min(240,len(y)//2)
                    if fade:
                        ramp=np.linspace(0,1,fade)
                        y[:fade]*=ramp;y[-fade:]*=ramp[::-1]
                    y=y.astype('<f4')
                    for i in range(0,len(y),2048):frame(0,y[i:i+2048].tobytes())
            asyncio.run(generate())
            frame(1)
        except Exception as e:
            frame(2,str(e).encode('utf-8'))
    return 0

if __name__=='__main__':sys.exit(main())
