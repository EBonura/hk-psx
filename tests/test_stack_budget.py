import sys
import struct
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
from stack_budget import main_frame_bytes, prepare_linker


class StackBudgetTests(unittest.TestCase):
    def test_reads_small_and_large_frames_from_final_exe(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'game.exe'
            for words, expected in [((0x27bdff80, 0), 128),
                                    ((0x340181c0, 0x03a1e823), 33216)]:
                data = bytearray(2056)
                struct.pack_into('<I', data, 0x18, 0x80010000)
                struct.pack_into('<II', data, 2048, *words)
                path.write_bytes(data)
                self.assertEqual(main_frame_bytes(path, '80010000 80010000 8 1 main'), expected)
            with self.assertRaisesRegex(ValueError, 'Missing main'):
                main_frame_bytes(path, '')

    def test_does_not_modify_sdk_and_content_change_forces_new_linker_path(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            sdk = root / '.psoxide/sdk/psoxide.ld'
            sdk.parent.mkdir(parents=True)
            original = 'STACK_RESERVE = 0x8000; /* SDK */\nLENGTH = RAM_SIZE - BIOS_SIZE - STACK_RESERVE\n'
            sdk.write_text(original)
            first = prepare_linker(root, root)
            self.assertEqual(sdk.read_text(), original)
            self.assertIn('STACK_RESERVE = 0xc000;', first.read_text())
            sdk.write_text(original + '/* other SDK change */\n')
            self.assertNotEqual(first, prepare_linker(root, root))
            sdk.write_text('STACK_RESERVE = 0x4000;\n')
            with self.assertRaisesRegex(ValueError, 'declaration changed'):
                prepare_linker(root, root)
