# Third-Party Notices

Meetily bundles no model weights in the repository or in released binaries. This file
records attribution for third-party machine-learning models the app may download at
runtime, on demand, with the user's consent.

## Speaker diarization models

Downloaded only after the user explicitly agrees to the in-app prompt (see
`docs/adr/0005-diarization-models-ungated-mirrors-and-consent-gate.md`). Both are ONNX
conversions, published by `csukuangfj` via the `k2-fsa/sherpa-onnx` project, of models
originally published by `pyannote.audio`.

### Speaker embedding model — CC BY 4.0

- **Model**: WeSpeaker ResNet34-LM (VoxCeleb)
- **Original**: [`pyannote/wespeaker-voxceleb-resnet34-LM`](https://huggingface.co/pyannote/wespeaker-voxceleb-resnet34-LM)
- **Copyright**: © pyannote.audio contributors
- **License**: [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/)
- **ONNX conversion source**: `k2-fsa/sherpa-onnx` GitHub release asset
  `wespeaker_en_voxceleb_resnet34_LM.onnx` (release tag `speaker-recongition-models`)

This notice constitutes attribution as required by the CC BY 4.0 license. No changes were
made to the model weights; only a runtime download and inference path were added.

### Speaker segmentation model — MIT

- **Model**: pyannote segmentation-3.0
- **Original**: [`pyannote/segmentation-3.0`](https://huggingface.co/pyannote/segmentation-3.0)
- **Copyright**: © 2022 CNRS
- **License**: MIT
- **ONNX conversion source**: `csukuangfj/sherpa-onnx-pyannote-segmentation-3-0` on
  Hugging Face
