"""数值视觉门槛的反例：空白背景不能掩盖整段文字缺失。"""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from PIL import Image, ImageDraw

class ReferenceGateTests(unittest.TestCase):
    def check(self, missing=False, shifted=False):
        with tempfile.TemporaryDirectory(prefix='layerlens-reference-') as temp:
            root = Path(temp)
            ref = Image.new('RGB',(100,100),'white')
            ImageDraw.Draw(ref).rectangle((20,20,30,24),fill='black')
            ref.save(root/'reference.png')
            actual=Image.new('RGB',(100,100),'white')
            if not missing:
                ImageDraw.Draw(actual).rectangle((20,24,30,28) if shifted else (20,20,30,24),fill='black')
            actual.save(root/'render-konva-100.png')
            report={'passed':True,'sourceHashes':['a'*64],'images':[{'name':'render-konva-100.png','unchanged':True,'sourceSha256':'a'*64,'engine':'konva','dpr':1,'canvas':{'documentWidth':100,'documentHeight':100,'zoom':1,'image':{'x':0,'y':0},'viewport':{'x':0,'y':0,'width':100,'height':100}}}]}
            (root/'report.json').write_text(json.dumps(report),encoding='utf8')
            result=subprocess.run([sys.executable,str(Path(__file__).with_name('compare-render-reference.py')),'--report',str(root/'report.json'),'--reference',str(root/'reference.png'),'--sha256',hashlib.sha256((root/'reference.png').read_bytes()).hexdigest(),'--acceptance','text'],capture_output=True,text=True)
            measured=json.loads((root/'visual-reference.json').read_text(encoding='utf8'))
            return result.returncode, measured['acceptance']['passed']
    def test_identical_ink_passes(self):
        self.assertEqual(self.check(),(0,True))
    def test_blank_output_fails_even_with_small_overall_error(self):
        self.assertEqual(self.check(missing=True),(1,False))
    def test_baseline_displacement_fails(self):
        self.assertEqual(self.check(shifted=True),(1,False))

if __name__ == '__main__':
    unittest.main()
