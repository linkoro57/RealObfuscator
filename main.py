"""Command-line interface for RealObfuscator, a Luau obfuscator for Roblox."""
from pathlib import Path
import os
import tempfile
import time
import click
from config import ObfuscationConfig, ProtectionLevel, DeadCodeDensity, lightweight_config
from obfuscator import Obfuscator
from version import __version__


def show_all_help(ctx, param, value):
    if not value or ctx.resilient_parsing:
        return
    hidden = [option for option in ctx.command.params
              if isinstance(option, click.Option) and option.hidden]
    try:
        for option in hidden:
            option.hidden = False
        click.echo(ctx.get_help())
    finally:
        for option in hidden:
            option.hidden = True
    ctx.exit()


@click.command(context_settings={'help_option_names': ['-h', '--help']})
@click.argument('source', required=False, type=click.Path(path_type=Path))
@click.option('--output', '-o', 'output_path', type=click.Path(path_type=Path),
              help='Output path. Default: NAME.obf.luau, or DIRECTORY-obfuscated.')
@click.option('--mode', type=click.Choice(['lightweight', 'rename', 'native', 'vm']),
              help='lightweight (default), rename, native, or vm (max profile).')
@click.option('--vm-rolling/--vm-cached', default=None,
              help='VM bytecode reads: rolling (default), or cached per function. Requires VM mode.')
@click.option('--vm-packed/--vm-unpacked', default=None,
              help='Compact VM bytecode representation (default for hardened VM profiles).')
@click.option('--quiet', '-q', is_flag=True, help='Show errors only.')
@click.version_option(__version__, prog_name='RealObfuscator')
@click.option('--help-all', is_flag=True, is_eager=True, expose_value=False,
              callback=show_all_help, help='Show individual settings and older flags.')
@click.option('--input', '-i', 'input_path', type=click.Path(path_type=Path),
              hidden=True, help='Input file or directory; alternative to SOURCE.')
@click.option('--level', '-l', type=click.IntRange(1, 4), hidden=True,
              help='Presets: 1=basic native, 2=control flow, 3=native checks, 4=VM max.')
@click.option('--vm', is_flag=True, hidden=True, help='Use VM execution (basic profile by default).')
@click.option('--antitamper', is_flag=True, hidden=True, help='Enable native anti-tamper checks.')
@click.option('--strings/--no-strings', default=None, hidden=True, help='Enable/disable native string encoding.')
@click.option('--cff/--no-cff', default=None, hidden=True, help='Enable/disable native control-flow flattening.')
@click.option('--deadcode/--no-deadcode', default=None, hidden=True, help='Enable/disable native dead code.')
@click.option('--seed', type=int, hidden=True, help='Development seed for reproducible output. Omit for releases.')
@click.option('--recursive', '-r', is_flag=True, hidden=True, help='Accepted for older commands; directories recurse automatically.')
@click.option('--density', type=click.Choice(['low', 'medium', 'high']), hidden=True,
              help='Native dead-code density (default: medium).')
@click.option('--vm-hardening', type=click.Choice(['basic', 'client-max', 'max']), hidden=True,
              help='VM profile. Requires --mode vm, --vm, or --level 4.')
@click.option('--lightweight', is_flag=True, hidden=True, help='Alias for --mode lightweight.')
@click.option('--rename-only', is_flag=True, hidden=True, help='Alias for --mode rename.')
def main(source, input_path, output_path, mode, level, vm, antitamper, strings,
         cff, deadcode, seed, recursive, density, vm_hardening, vm_rolling, vm_packed,
         lightweight, rename_only, quiet):
    """Obfuscate a Luau file or directory for Roblox.

    Example: python main.py script.luau --mode vm

    Keep the original source. Test generated code in Roblox Studio.
    """
    if source is not None and input_path is not None:
        raise click.UsageError('Use SOURCE or --input, not both.')
    input_p = source if source is not None else input_path
    if input_p is None:
        raise click.UsageError('Provide a Luau file or directory. See --help.')
    if not input_p.exists():
        raise click.ClickException(f'Input path not found: {input_p}')
    if not input_p.is_file() and not input_p.is_dir():
        raise click.ClickException(f'Input must be a file or directory: {input_p}')

    primary_modes = sum([mode is not None, level is not None, lightweight, rename_only])
    if primary_modes > 1:
        raise click.UsageError('Choose one of --mode, --level, --lightweight, or --rename-only.')
    if mode is not None and vm:
        raise click.UsageError('Use --mode vm or --vm, not both.')
    if lightweight:
        mode = 'lightweight'
    elif rename_only:
        mode = 'rename'
    custom_native = antitamper or strings is not None or cff is not None or deadcode is not None or density is not None
    if mode is None and level is None and not vm and not custom_native:
        mode = 'lightweight'

    if mode in ('lightweight', 'rename'):
        if vm or vm_rolling is not None or vm_packed is not None or antitamper or vm_hardening is not None or cff or deadcode or density is not None:
            raise click.UsageError('Lightweight/rename modes cannot enable VM, anti-tamper, control-flow, or dead-code options.')
        config = lightweight_config(seed=seed)
        if mode == 'rename':
            if strings:
                raise click.UsageError('--mode rename cannot enable --strings.')
            config.encrypt_strings = False
        elif strings is not None:
            config.encrypt_strings = strings
    else:
        selected_level = 4 if mode == 'vm' else 3 if mode == 'native' else level
        config = ObfuscationConfig(seed=seed,
            level=ProtectionLevel(selected_level) if selected_level is not None else None,
            virtualize=vm, vm_hardening=vm_hardening)
        if vm:
            config.virtualize = True
        if antitamper:
            config.anti_tamper = True
        for attribute, value in [('encrypt_strings', strings), ('control_flow_flatten', cff),
                                 ('inject_dead_code', deadcode)]:
            if value is not None:
                setattr(config, attribute, value)
        if density is not None:
            config.dead_code_density = DeadCodeDensity(density)
    if vm_hardening is not None and not config.virtualize:
        raise click.UsageError('--vm-hardening requires --mode vm, --vm, or --level 4.')
    if vm_rolling is not None:
        if not config.virtualize:
            raise click.UsageError('--vm-rolling/--vm-cached requires --mode vm, --vm, or --level 4.')
        config.vm_predecode_bytecode = not vm_rolling
    if vm_packed is not None:
        if not config.virtualize:
            raise click.UsageError('--vm-packed/--vm-unpacked requires --mode vm, --vm, or --level 4.')
        config.vm_pack_bytecode = vm_packed

    directory_name = input_p.resolve() if input_p.is_dir() else None
    output_suffix = input_p.suffix if input_p.suffix.lower() in {'.lua', '.luau'} else '.luau'
    output_p = output_path or (directory_name.with_name(directory_name.name + '-obfuscated') if directory_name
                              else input_p.with_name(input_p.stem + '.obf' + output_suffix))
    input_absolute, output_absolute = input_p.resolve(), output_p.resolve()
    if input_absolute == output_absolute:
        raise click.UsageError('Output must differ from input; keep your original source.')
    if input_p.is_dir():
        if input_absolute in output_absolute.parents:
            raise click.UsageError('Keep the output directory outside the input directory.')
        if output_p.exists() and not output_p.is_dir():
            raise click.UsageError('Directory input needs a directory output.')
    elif output_p.is_dir():
        raise click.UsageError('File input needs a file output.')

    started = time.perf_counter()
    obfuscator = Obfuscator(config)
    if input_p.is_dir():
        processed, failed = process_directory(input_p, output_p, obfuscator, quiet)
        if not quiet:
            click.echo(f'{processed} files written to {output_p}; {failed} failed ({time.perf_counter() - started:.2f}s).')
    else:
        processed, failed = process_file(input_p, output_p, obfuscator, quiet)
    if failed:
        raise click.exceptions.Exit(1)


def process_file(input_path: Path, output_path: Path,
                 obfuscator: Obfuscator, quiet: bool) -> tuple:
    """Generate and atomically replace output so failures preserve old files."""
    try:
        source = input_path.read_text(encoding='utf-8')
        result = obfuscator.obfuscate(source)
        output_path.parent.mkdir(parents=True, exist_ok=True)
        write_atomic(output_path, result)
        if not quiet:
            click.echo(f'{input_path} -> {output_path} ({len(source.encode("utf-8")):,} -> {len(result.encode("utf-8")):,} bytes)')
        return 1, 0
    except Exception as error:
        click.echo(f'Error: {input_path}: {error}', err=True)
        return 0, 1


def write_atomic(output_path: Path, content: str):
    """Write UTF-8 text beside its destination and replace it in one operation."""
    temporary_path = None
    try:
        with tempfile.NamedTemporaryFile(
                mode='w', encoding='utf-8', newline='', delete=False,
                dir=output_path.parent, prefix=f'.{output_path.name}.', suffix='.tmp') as file:
            temporary_path = Path(file.name)
            file.write(content)
            file.flush()
            os.fsync(file.fileno())
        os.replace(temporary_path, output_path)
    finally:
        if temporary_path is not None:
            temporary_path.unlink(missing_ok=True)


def process_directory(input_dir: Path, output_dir: Path,
                      obfuscator: Obfuscator, quiet: bool) -> tuple:
    """Preserve relative paths while processing both Luau file extensions."""
    files = [path for path in sorted(input_dir.rglob('*'))
             if path.is_file() and path.suffix.lower() in {'.lua', '.luau'}
             and not path.stem.lower().endswith('.obf')]
    if not files:
        raise click.ClickException(f'No .lua or .luau files found in {input_dir}')
    total_ok = total_fail = 0
    for source in files:
        ok, fail = process_file(source, output_dir / source.relative_to(input_dir), obfuscator, quiet)
        total_ok += ok
        total_fail += fail
    return total_ok, total_fail


if __name__ == '__main__':
    main()
