"""Check simplified commands, legacy presets, and preservation of input files."""
from pathlib import Path
import unittest

from click.testing import CliRunner
from config import ObfuscationConfig, ProtectionLevel, lightweight_config
from main import main
from obfuscator import Obfuscator
from test_luau import LUAU, run


class CliTests(unittest.TestCase):
    def test_default_and_named_modes(self):
        runner = CliRunner()
        source = 'local text="héllo";local function f(n) return text,n+10 end;return f(5)'
        with runner.isolated_filesystem():
            Path('script with spaces.luau').write_text(source, encoding='utf-8')
            rename = lightweight_config(seed=12345)
            rename.encrypt_strings = False
            modes = [(None, lightweight_config(seed=12345)),
                     ('rename', rename),
                     ('native', ObfuscationConfig(level=ProtectionLevel.MAXIMUM, seed=12345)),
                     ('vm', ObfuscationConfig(level=ProtectionLevel.PARANOID, seed=12345))]
            for mode, config in modes:
                with self.subTest(mode=mode):
                    args = ['script with spaces.luau', '--seed', '12345']
                    if mode:
                        args += ['--mode', mode]
                    result = runner.invoke(main, args)
                    self.assertEqual(result.exit_code, 0, result.output)
                    protected = Path('script with spaces.obf.luau').read_text(encoding='utf-8')
                    self.assertEqual(protected, Obfuscator(config).obfuscate(source))
                    self.assertEqual(Path('script with spaces.luau').read_text(encoding='utf-8'), source)
                    if LUAU.is_file():
                        self.assertEqual(run(protected), run(source))
            result = runner.invoke(main, ['script with spaces.luau', '--quiet'])
            self.assertEqual(result.exit_code, 0, result.output)
            self.assertEqual(result.output, '')
            Path('Pasted text.txt').write_text(source, encoding='utf-8')
            pasted = runner.invoke(main, ['-i', 'Pasted text.txt', '--mode', 'vm', '--quiet'])
            self.assertEqual(pasted.exit_code, 0, pasted.output)
            self.assertTrue(Path('Pasted text.obf.luau').is_file())

    def test_legacy_presets_and_rolling(self):
        runner = CliRunner()
        with runner.isolated_filesystem():
            Path('input.lua').write_text('return "unchanged",17', encoding='utf-8')
            for old, new in [(['--lightweight'], ['--mode', 'lightweight']),
                             (['--rename-only'], ['--mode', 'rename']),
                             (['--level', '3'], ['--mode', 'native']),
                             (['--level', '4', '--vm-hardening', 'max'], ['--mode', 'vm']),
                             (['--level', '4', '--vm-rolling'], ['--mode', 'vm', '--vm-rolling']),
                             (['--level', '4', '--vm-cached'], ['--mode', 'vm', '--vm-cached']),
                             (['--mode', 'vm'], ['--mode', 'vm', '--vm-rolling'])]:
                with self.subTest(old=old):
                    legacy = runner.invoke(main, ['-i', 'input.lua', '-o', 'old.lua', '--seed', '67890', *old])
                    simple = runner.invoke(main, ['input.lua', '-o', 'new.lua', '--seed', '67890', *new])
                    self.assertEqual(legacy.exit_code, 0, legacy.output)
                    self.assertEqual(simple.exit_code, 0, simple.output)
                    self.assertEqual(Path('old.lua').read_bytes(), Path('new.lua').read_bytes())
            cached = ObfuscationConfig(level=ProtectionLevel.PARANOID,
                                       vm_predecode_bytecode=True, seed=67890)
            result = runner.invoke(main, ['input.lua', '-o', 'cached.lua', '--mode', 'vm',
                                          '--vm-cached', '--seed', '67890'])
            self.assertEqual(result.exit_code, 0, result.output)
            self.assertEqual(Path('cached.lua').read_text(encoding='utf-8'),
                             Obfuscator(cached).obfuscate(Path('input.lua').read_text(encoding='utf-8')))
            self.assertFalse(ObfuscationConfig(virtualize=True).vm_predecode_bytecode)

    def test_directory_paths_and_partial_failure(self):
        runner = CliRunner()
        with runner.isolated_filesystem():
            Path('scripts/sub').mkdir(parents=True)
            Path('scripts/one.lua').write_text('return 1', encoding='utf-8')
            Path('scripts/sub/two.luau').write_text('return 2', encoding='utf-8')
            Path('scripts/notes.txt').write_text('not source', encoding='utf-8')
            generated = runner.invoke(main, ['scripts/one.lua', '--quiet'])
            self.assertEqual(generated.exit_code, 0, generated.output)
            result = runner.invoke(main, ['scripts', '--quiet'])
            self.assertEqual(result.exit_code, 0, result.output)
            self.assertEqual({str(p.relative_to('scripts-obfuscated')).replace('\\', '/')
                              for p in Path('scripts-obfuscated').rglob('*') if p.is_file()},
                             {'one.lua', 'sub/two.luau'})
            Path('scripts/bad.lua').write_text('return `unsupported`', encoding='utf-8')
            result = runner.invoke(main, ['scripts', '-o', 'dist', '--quiet'])
            self.assertEqual(result.exit_code, 1, result.output)
            self.assertIn('bad.lua', result.output)
            self.assertFalse(Path('dist/bad.lua').exists())
            self.assertTrue(Path('dist/sub/two.luau').is_file())
            Path('empty').mkdir()
            self.assertEqual(runner.invoke(main, ['empty', '--quiet']).exit_code, 1)

    def test_rejected_paths_and_options_preserve_files(self):
        runner = CliRunner()
        with runner.isolated_filesystem():
            source = 'return 17'
            Path('input.lua').write_text(source, encoding='utf-8')
            Path('scripts').mkdir()
            Path('input.txt').write_text(source, encoding='utf-8')
            invalid = [[], ['input.lua', '-i', 'input.lua'], ['input.lua', '-o', './input.lua'],
                       ['scripts', '-o', 'scripts/output'], ['scripts', '-o', 'input.lua'],
                       ['input.lua', '-o', 'scripts'],
                       ['input.lua', '--mode', 'vm', '--level', '4'],
                       ['input.lua', '--mode', 'native', '--vm'],
                       ['input.lua', '--mode', 'rename', '--strings'],
                       ['input.lua', '--vm-rolling'], ['input.lua', '--vm-cached'],
                       ['input.lua', '--vm-hardening', 'max']]
            for args in invalid:
                with self.subTest(args=args):
                    self.assertEqual(runner.invoke(main, args).exit_code, 2)
                    self.assertEqual(Path('input.lua').read_text(encoding='utf-8'), source)
            self.assertEqual(runner.invoke(main, ['missing.lua', '--quiet']).exit_code, 1)
            Path('bad.lua').write_text('return `unsupported`', encoding='utf-8')
            Path('keep.lua').write_text('keep me', encoding='utf-8')
            result = runner.invoke(main, ['bad.lua', '-o', 'keep.lua', '--quiet'])
            self.assertEqual(result.exit_code, 1, result.output)
            self.assertEqual(Path('keep.lua').read_text(encoding='utf-8'), 'keep me')

    def test_help_and_version(self):
        runner = CliRunner()
        common = runner.invoke(main, ['--help'])
        full = runner.invoke(main, ['--help-all'])
        repeated = runner.invoke(main, ['--help'])
        self.assertEqual(common.exit_code, 0, common.output)
        self.assertEqual(full.exit_code, 0, full.output)
        self.assertEqual(common.output, repeated.output)
        self.assertIn('--mode', common.output)
        self.assertNotIn('--level', common.output)
        self.assertIn('--level', full.output)
        self.assertIn('--vm-hardening', full.output)
        self.assertIn('--vm-cached', common.output)
        self.assertIn('1.1.0', runner.invoke(main, ['--version']).output)
