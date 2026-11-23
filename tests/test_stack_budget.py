import os
import subprocess
import sys
import struct
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'host'))
import stack_budget
import code_modules
from stack_budget import main_frame_bytes, prepare_linker


class StackBudgetTests(unittest.TestCase):
    def test_ram_top_is_the_one_value_the_link_and_the_module_image_share(self):
        self.assertEqual(stack_budget.RAM_TOP, 0x80000000 + stack_budget.RAM_SIZE)
        self.assertEqual(stack_budget.STACK_INIT, stack_budget.RAM_TOP - 0x100)
        self.assertEqual(code_modules.STACK_FLOOR, stack_budget.STACK_INIT - stack_budget.STACK_RESERVE)
        self.assertEqual(stack_budget.RAM_SIZE, 0x200000 - stack_budget.CORE_WINDOW_BYTES)

    def test_ram_top_for_the_window_sizes(self):
        host = Path(__file__).resolve().parents[1] / 'host'
        for window, size, top, init in [('0', 0x200000, 0x80200000, 0x801FFF00),
                                        ('8192', 0x1FE000, 0x801FE000, 0x801FDF00),
                                        ('10240', 0x1FD800, 0x801FD800, 0x801FD700)]:
            env = dict(os.environ, HK_CORE_WINDOW_BYTES=window, PYTHONPATH=str(host))
            out = subprocess.run([sys.executable, '-c', 'import stack_budget as s; print(s.RAM_SIZE, s.RAM_TOP, s.STACK_INIT)'],
                                 env=env, capture_output=True, text=True, check=True).stdout.split()
            self.assertEqual([int(x) for x in out], [size, top, init], window)

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
            original = ('RAM_SIZE = DEFINED(RAM_SIZE_OVERRIDE) ? RAM_SIZE_OVERRIDE : 2M; /* SDK */\n'
                        'STACK_RESERVE = DEFINED(STACK_RESERVE_OVERRIDE) ? STACK_RESERVE_OVERRIDE : 0x8000; /* SDK */\n'
                        'LENGTH = EXE_HEAD_BYTES + RAM_SIZE - BIOS_SIZE - STACK_RESERVE\n')
            sdk.write_text(original)
            first = prepare_linker(root, root)
            self.assertEqual(sdk.read_text(), original)
            self.assertTrue(first.read_text().startswith(f'RAM_SIZE_OVERRIDE = {stack_budget.RAM_SIZE:#x};'))
            self.assertIn('STACK_RESERVE_OVERRIDE = 0xc000;', first.read_text())
            self.assertTrue(first.read_text().endswith(original))
            sdk.write_text(original + '/* other SDK change */\n')
            self.assertNotEqual(first, prepare_linker(root, root))
            sdk.write_text('STACK_RESERVE = 0x4000;\n')
            with self.assertRaisesRegex(ValueError, 'no STACK_RESERVE_OVERRIDE hook'):
                prepare_linker(root, root)
