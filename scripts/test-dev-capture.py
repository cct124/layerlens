"""独立已知 RGBA 验证分块拼接；缺块、错位或篡改不能生成成功截图。"""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from PIL import Image

spec = importlib.util.spec_from_file_location('capture', Path(__file__).with_name('assemble-dev-capture.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class CaptureTests(unittest.TestCase):
    def test_integrity_and_rejection(self):
        with tempfile.TemporaryDirectory(prefix='layerlens-capture-') as temporary:
            directory=Path(temporary)
            raw=bytes(i%251 for i in range(300*260*4))
            original=Image.frombytes('RGBA',(300,260),raw)
            data={'captureId':'fixture','width':300,'height':260,'tileSize':256,'tileCount':4,
                  'rgbaSha256':hashlib.sha256(raw).hexdigest(),'tiles':[]}
            for index in range(4):
                x,y=index%2*256,index//2*256
                width,height=min(256,300-x),min(256,260-y)
                name=f'{index}.png'
                original.crop((x,y,x+width,y+height)).save(directory/name)
                data['tiles'].append({'captureId':'fixture','index':index,'x':x,'y':y,'width':width,'height':height,'file':name})
            manifest=directory/'manifest.json'
            manifest.write_text(json.dumps(data),encoding='utf8')
            module.assemble(manifest,directory/'output.png')
            with Image.open(directory/'output.png') as image:
                self.assertEqual(image.convert('RGBA').tobytes(),raw)
            for mutate in ['missing','hash','position']:
                invalid=json.loads(json.dumps(data))
                if mutate=='missing': invalid['tiles'].pop()
                elif mutate=='hash': invalid['rgbaSha256']='0'*64
                else: invalid['tiles'][1]['x']=0
                manifest.write_text(json.dumps(invalid),encoding='utf8')
                target=directory/(mutate+'.png')
                with self.assertRaises(ValueError): module.assemble(manifest,target)
                self.assertFalse(target.exists())

if __name__=='__main__': unittest.main()
