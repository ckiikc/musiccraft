"""Document associations shared by packaging, local installation and QA."""
import plistlib
import sys
from pathlib import Path

FORMATS = [
    ('FLAC audio', 'org.xiph.flac', ['flac'], 'audio/flac'),
    ('WAVE audio', 'com.microsoft.waveform-audio', ['wav'], 'audio/wav'),
    ('AIFF audio', 'public.aiff-audio', ['aif', 'aiff'], 'audio/aiff'),
    ('MPEG-4 audio', 'public.mpeg-4-audio', ['m4a'], 'audio/mp4'),
    ('Apple Lossless audio', 'io.github.ckiikc.musiccraft.alac', ['alac'], 'audio/x-alac'),
    ('MP3 audio', 'public.mp3', ['mp3'], 'audio/mpeg'),
    ('AAC audio', 'public.aac-audio', ['aac'], 'audio/aac'),
    ('Ogg audio', 'org.xiph.ogg-audio', ['ogg', 'oga'], 'audio/ogg'),
    ('MPEG-4 media', 'public.mpeg-4', ['mp4'], 'video/mp4'),
]

def apply(info):
    info['CFBundleDocumentTypes'] = [dict(CFBundleTypeName=name, CFBundleTypeRole='Viewer',
        LSHandlerRank='Alternate', LSItemContentTypes=[uti], CFBundleTypeExtensions=exts)
        for name, uti, exts, mime in FORMATS]
    info['UTImportedTypeDeclarations'] = [dict(UTTypeIdentifier=uti,
        UTTypeDescription=name, UTTypeConformsTo=['public.audio'],
        UTTypeTagSpecification={'public.filename-extension':exts, 'public.mime-type':mime})
        for name, uti, exts, mime in FORMATS if uti in ['org.xiph.flac', 'org.xiph.ogg-audio', 'io.github.ckiikc.musiccraft.alac']]
    return info

if __name__ == '__main__':
    path = Path(sys.argv[1]) / 'Contents/Info.plist'
    path.write_bytes(plistlib.dumps(apply(plistlib.loads(path.read_bytes()))))
