import runpy
import unittest
import re
from pathlib import Path
HERE = Path(__file__).parent
m = runpy.run_path(str(HERE/'document-types.py'))
class Associations(unittest.TestCase):
    def test_formats_match_engine_without_claiming_all_documents_or_changing_defaults(self):
        source=(HERE.parent/'src/library.rs').read_text().split('pub fn audio_path')[1].split('pub fn playlist')[0]
        extensions=set(re.findall(r'"([a-z0-9]+)"',source))
        info=m['apply']({'CFBundleIdentifier':'test'})
        self.assertEqual(extensions,{x for t in info['CFBundleDocumentTypes'] for x in t['CFBundleTypeExtensions']})
        self.assertTrue(all(t['CFBundleTypeRole']=='Viewer' and t['LSHandlerRank']=='Alternate' for t in info['CFBundleDocumentTypes']))
        self.assertEqual(info['CFBundleIdentifier'],'test')
if __name__=='__main__': unittest.main()
