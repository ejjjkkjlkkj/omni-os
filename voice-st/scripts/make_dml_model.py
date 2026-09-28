"""Rewrite Kokoro's grouped ConvTranspose nodes into ops DirectML supports.

DirectML rejects grouped 1-D ConvTranspose (E_INVALIDARG on /N.1/pool etc.).
For stride 2, kernel 3, pads (1,1), output_padding 1 (the only grouped case in
Kokoro v1.0) the op equals: interleave zeros (x0,0,x1,0,...), pad 1 left and
1 right, then a grouped Conv with the kernel flipped. Output length 2L either way.

Usage: python scripts/make_dml_model.py IN.onnx OUT.onnx
Verified numerically by scripts/make_dml_model.py --check (CPU, both graphs).
"""
import sys
import numpy as np
import onnx
from onnx import helper, numpy_helper, TensorProto


def rewrite(model):
    g = model.graph
    inits = {i.name: i for i in g.initializer}
    nodes, rewritten = [], 0
    const = lambda name, arr: g.initializer.append(numpy_helper.from_array(np.asarray(arr), name))
    const('dmlfix_zero', np.array(0.0, dtype=np.float32))
    const('dmlfix_axis3', np.array([3], dtype=np.int64))
    const('dmlfix_shape', np.array([0, 0, -1], dtype=np.int64))
    const('dmlfix_pads', np.array([0, 0, 1, 0, 0, 1], dtype=np.int64))
    for n in g.node:
        a = {x.name: helper.get_attribute_value(x) for x in n.attribute}
        if not (n.op_type == 'ConvTranspose' and a.get('group', 1) > 1):
            nodes.append(n)
            continue
        assert a['strides'] == [2] and a['kernel_shape'] == [3] and a['pads'] == [1, 1] and a.get('output_padding') == [1], a
        p = n.name.replace('/', '_')
        w = numpy_helper.to_array(inits[n.input[1]])
        const(p + '_wflip', np.ascontiguousarray(w[:, :, ::-1]))
        x, y = n.input[0], n.output[0]
        nodes += [
            helper.make_node('Unsqueeze', [x, 'dmlfix_axis3'], [p + '_u'], name=p + '_u'),
            helper.make_node('Mul', [p + '_u', 'dmlfix_zero'], [p + '_z'], name=p + '_z'),
            helper.make_node('Concat', [p + '_u', p + '_z'], [p + '_c'], axis=3, name=p + '_c'),
            helper.make_node('Reshape', [p + '_c', 'dmlfix_shape'], [p + '_r'], name=p + '_r'),
            helper.make_node('Pad', [p + '_r', 'dmlfix_pads'], [p + '_p'], mode='constant', name=p + '_p'),
            helper.make_node('Conv', [p + '_p', p + '_wflip'] + list(n.input[2:]), [y], group=a['group'],
                             kernel_shape=[3], strides=[1], pads=[0, 0], dilations=[1], name=p + '_conv'),
        ]
        rewritten += 1
    del g.node[:]
    g.node.extend(nodes)
    return rewritten


def check(orig, fixed):
    import onnxruntime as rt, sys as _s
    _s.path.insert(0, 'neural')
    from kokoro_onnx import Kokoro
    outs = []
    for path in (orig, fixed):
        k = Kokoro.from_session(rt.InferenceSession(path, providers=['CPUExecutionProvider']), 'neural/models/voices-v1.0.bin')
        outs.append(k.create('Bonjour, bienvenue dans ST.', voice='ff_siwis', lang='fr-fr')[0])
    a, b = outs
    print('lengths', len(a), len(b), 'max abs diff', float(np.max(np.abs(a - b))) if len(a) == len(b) else 'n/a')


if __name__ == '__main__':
    if sys.argv[1] == '--check':
        check(sys.argv[2], sys.argv[3])
    else:
        m = onnx.load(sys.argv[1])
        print('rewritten', rewrite(m))
        onnx.checker.check_model(m)
        onnx.save(m, sys.argv[2])
