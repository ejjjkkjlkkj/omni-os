"""Reproducible FR/EN benchmark. Never removes previous runs; errors fail the run."""
from pathlib import Path
import argparse, hashlib, json, platform, subprocess, sys, time
import numpy as np
import scipy
import scipy.signal as signal
import soundfile as sf
import parselmouth as pm
from parselmouth.praat import call

ROOT = Path(__file__).resolve().parent
PROJECT = ROOT.parent

def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def finite(x):
    return float(x) if np.isfinite(x) else None

def db(x):
    return float(20*np.log10(max(float(x), 1e-12)))

def analyze(path):
    info = sf.info(path)
    x, sr = sf.read(path, always_2d=True)
    assert len(x) > sr//10 and np.isfinite(x).all(), 'Invalid/short audio'
    mono = x.mean(axis=1)
    assert np.max(abs(mono)) > 0.001, 'Silent audio'
    from math import gcd
    g = gcd(sr, 24000)
    y = signal.resample_poly(mono, 24000//g, sr//g)
    snd = pm.Sound(y, sampling_frequency=24000)
    pitch = snd.to_pitch_ac(time_step=.01, pitch_floor=60, pitch_ceiling=400,
                           silence_threshold=.03, voicing_threshold=.45)
    f0 = pitch.selected_array['frequency']
    voiced = f0 > 0
    harmon = snd.to_harmonicity_cc(time_step=.01, minimum_pitch=60,
                                 silence_threshold=.1, periods_per_window=1)
    hv = harmon.values[0]
    valid = hv > -199
    vi = np.array([pitch.get_value_at_time(t) for t in harmon.xs()])
    voiced_h = valid & np.isfinite(vi) & (vi > 0)
    pulses = call(snd, 'To PointProcess (periodic, cc)', 60, 400)
    jitter = call(pulses, 'Get jitter (local)', 0, 0, 1/400, 1/60, 1.3)
    both = voiced[1:] & voiced[:-1]
    steps = abs(12*np.log2(f0[1:][both]/f0[:-1][both]))
    frames = np.lib.stride_tricks.sliding_window_view(y, 960)[::480]
    rms_frames = np.sqrt(np.mean(frames*frames, axis=1))
    levels = 20*np.log10(np.maximum(rms_frames, 1e-7))
    active = levels > max(-60, float(levels.max())-40)
    f, _, power = signal.spectrogram(y, fs=24000, nperseg=1024, noverlap=512)
    weights = power.sum(axis=0)
    centroid = (power*f[:,None]).sum(axis=0)/np.maximum(weights, 1e-20)
    spectral_active = weights > weights.max()*1e-4
    rms = np.sqrt(np.mean(mono*mono))
    return dict(duration_s=len(mono)/sr, sample_rate=sr, channels=info.channels,
        subtype=info.subtype, file_bytes=Path(path).stat().st_size, sha256=sha(path),
        peak_dbfs=db(np.max(abs(mono))), rms_dbfs=db(rms),
        crest_db=db(np.max(abs(mono)))-db(rms), clipping_ratio=float(np.mean(abs(mono)>=.999)),
        active_dynamic_db=float(np.percentile(levels[active],95)-np.percentile(levels[active],5)),
        active_ratio=float(np.mean(active)), voiced_ratio=float(np.mean(voiced)),
        f0_mean_hz=finite(np.mean(f0[voiced])) if voiced.any() else None,
        f0_std_hz=finite(np.std(f0[voiced])) if voiced.any() else None,
        f0_step_median_st=finite(np.median(steps)) if len(steps) else None,
        f0_step_p95_st=finite(np.percentile(steps,95)) if len(steps) else None,
        jitter_local_pct=finite(100*jitter),
        hnr_db=finite(np.mean(hv[voiced_h])) if voiced_h.any() else None,
        hnr_nonsilent_db=finite(np.mean(hv[valid])) if valid.any() else None,
        hnr_voiced_frames=int(voiced_h.sum()),
        spec_centroid_hz=finite(np.mean(centroid[spectral_active])))

def execute(cmd, timeout=120, engine_timing=None):
    t = time.perf_counter()
    p = subprocess.run(list(map(str,cmd)), capture_output=True, timeout=timeout)
    ms = (time.perf_counter()-t)*1000
    if p.returncode:
        raise RuntimeError(f'Exit {p.returncode}: {p.stderr.decode(errors="replace")}')
    # ST prints engine-internal timing: load_ms, first_audio_ms, synthesis_ms.
    for line in p.stderr.decode(errors='replace').splitlines():
        if line.startswith('ST_TIMING') and engine_timing is not None:
            engine_timing.append({k: float(v) for k, v in (x.split('=') for x in line.split()[1:])})
    return ms

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--tag', default='final')
    ap.add_argument('--engines', nargs='+', default=['st','sapi5','espeak'])
    ap.add_argument('--st-exe', type=Path, default=PROJECT/'target/release/st.exe')
    ap.add_argument('--repeat', type=int, default=5)
    ap.add_argument('--corpus', type=Path, default=ROOT/'corpus.json')
    a = ap.parse_args()
    assert a.repeat > 0 and a.tag.replace('-','').replace('_','').isalnum()
    folder = ROOT/'runs'/a.tag
    folder.mkdir(parents=True, exist_ok=True)
    espeak = ROOT/'tools/espeak-ng/espeak-ng.exe'
    corpus = json.loads(a.corpus.read_text(encoding='utf-8'))
    records, commands = [], []
    for lang, phrases in corpus['languages'].items():
        for ph in phrases:
            for engine in a.engines:
                out = folder/'wavs'/engine/lang/(ph['id']+'.wav')
                out.parent.mkdir(parents=True,exist_ok=True)
                rec = dict(engine=engine, stage=a.tag, lang=lang, phrase_id=ph['id'],
                           text=ph['text'], category=ph['category'], path=out.relative_to(ROOT).as_posix())
                if engine in ('st', 'st-neural'):
                    voice = 'female' if engine == 'st' else {'fr':'ff_siwis','en':'af_heart'}[lang]
                    cmd=[a.st_exe,'--text',ph['text'],'--lang',lang,'--voice',voice,'--out',out]
                    rec['binary_sha256']=sha(a.st_exe)
                    rec['binary_bytes']=a.st_exe.stat().st_size
                elif engine == 'espeak':
                    cmd=[espeak,'--path='+str(espeak.parent),'-v',lang,'-s','175','-w',out,ph['text']]
                    rec['binary_sha256']=sha(espeak)
                elif engine == 'sapi5':
                    cmd=['powershell.exe','-NoProfile','-ExecutionPolicy','Bypass','-File',ROOT/'sapi_gen.ps1',
                         '-VoiceName',{'fr':'Hortense','en':'Zira'}[lang],'-Text',ph['text'],
                         '-OutFile',out,'-SampleRate','24000']
                else:
                    raise ValueError(engine)
                try:
                    first = execute(cmd)
                    inner = []
                    times = [execute(cmd, engine_timing=inner) for _ in range(a.repeat)]
                    rec.update(analyze(out))
                    if inner:
                        for key in ('load_ms', 'first_audio_ms', 'synthesis_ms'):
                            rec['engine_'+key] = float(np.median([x[key] for x in inner]))
                        rec['engine_rtf'] = rec['engine_synthesis_ms']/1000/rec['duration_s']
                    rec.update(first_run_ms=first, synth_ms=float(np.median(times)), timing_ms=times,
                               rtf=float(np.median(times))/1000/rec['duration_s'], status='PASS')
                except Exception as e:
                    rec.update(status='FAIL',error=str(e))
                commands.append(list(map(str,cmd)))
                records.append(rec)
                print(a.tag, ph['id'], engine, rec['status'], round(rec.get('hnr_db') or 0,2), flush=True)
                (folder/'metrics.json').write_text(json.dumps(records,ensure_ascii=False,indent=2,allow_nan=False),encoding='utf-8')
    env=dict(python=sys.version,platform=platform.platform(),numpy=np.__version__,scipy=scipy.__version__,
             parselmouth=pm.__version__,praat=pm.PRAAT_VERSION,corpus=a.corpus.name,corpus_sha256=sha(a.corpus),
             analyzer_sha256=sha(__file__),commands=commands,repeat=a.repeat,
             timing='Wall-clock process + initialization + synthesis + WAV IO; not engine-only latency')
    (folder/'provenance.json').write_text(json.dumps(env,indent=2),encoding='utf-8')
    return int(any(r['status']=='FAIL' for r in records))

if __name__=='__main__':
    sys.exit(main())
