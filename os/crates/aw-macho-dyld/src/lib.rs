#![no_std]
#![forbid(unsafe_code)]

use aw_macho::MachSlice;

const MACH_HEADER_64_SIZE: usize = 32;
const LOAD_COMMAND_HEADER_SIZE: usize = 8;
const DYLINKER_COMMAND_SIZE: usize = 12;
const RPATH_COMMAND_SIZE: usize = 12;
const DYLIB_COMMAND_SIZE: usize = 24;

const LC_REQ_DYLD: u32 = 0x8000_0000;
const LC_LOAD_DYLIB: u32 = 0x0c;
const LC_LOAD_DYLINKER: u32 = 0x0e;
const LC_LOAD_WEAK_DYLIB: u32 = 0x18 | LC_REQ_DYLD;
const LC_RPATH: u32 = 0x1c | LC_REQ_DYLD;
const LC_REEXPORT_DYLIB: u32 = 0x1f | LC_REQ_DYLD;
const LC_LAZY_LOAD_DYLIB: u32 = 0x20;
const LC_LOAD_UPWARD_DYLIB: u32 = 0x23 | LC_REQ_DYLD;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DyldError {
    IntegerOverflow,
    SliceOutOfBounds,
    LoadCommandOutOfBounds {
        index: u32,
    },
    LoadCommandTooSmall {
        index: u32,
        size: u32,
    },
    CommandTooSmall {
        index: u32,
        command: u32,
        size: usize,
        minimum: usize,
    },
    StringOffsetOutOfBounds {
        index: u32,
        offset: u32,
        minimum: usize,
        size: usize,
    },
    UnterminatedPath {
        index: u32,
    },
    EmptyPath {
        index: u32,
    },
    DuplicateDynamicLinker,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DylibKind {
    Required,
    Weak,
    Reexport,
    Lazy,
    Upward,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Dylib<'a> {
    pub command_index: u32,
    pub kind: DylibKind,
    pub path: &'a [u8],
    pub timestamp: u32,
    pub current_version: u32,
    pub compatibility_version: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RunPath<'a> {
    pub command_index: u32,
    pub path: &'a [u8],
}

#[derive(Clone, Copy, Debug)]
struct LoadCommand<'a> {
    index: u32,
    command: u32,
    bytes: &'a [u8],
}

#[derive(Clone, Copy, Debug)]
struct CommandIter<'a> {
    image: &'a [u8],
    cursor: usize,
    commands_end: usize,
    next_index: u32,
    command_count: u32,
    failed: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct DylibIter<'a> {
    commands: CommandIter<'a>,
    failed: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct RunPathIter<'a> {
    commands: CommandIter<'a>,
    failed: bool,
}

fn checked_end(offset: usize, size: usize) -> Result<usize, DyldError> {
    offset.checked_add(size).ok_or(DyldError::IntegerOverflow)
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32, DyldError> {
    let end = checked_end(offset, 4)?;
    let data = bytes.get(offset..end).ok_or(DyldError::SliceOutOfBounds)?;
    Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
}

fn selected_image(bytes: &[u8], slice: MachSlice) -> Result<&[u8], DyldError> {
    let end = checked_end(slice.offset, slice.size)?;
    bytes
        .get(slice.offset..end)
        .ok_or(DyldError::SliceOutOfBounds)
}

fn commands(bytes: &[u8], slice: MachSlice) -> Result<CommandIter<'_>, DyldError> {
    let image = selected_image(bytes, slice)?;
    let commands_end = checked_end(MACH_HEADER_64_SIZE, slice.header.command_bytes as usize)?;
    if commands_end > image.len() {
        return Err(DyldError::SliceOutOfBounds);
    }

    Ok(CommandIter {
        image,
        cursor: MACH_HEADER_64_SIZE,
        commands_end,
        next_index: 0,
        command_count: slice.header.command_count,
        failed: false,
    })
}

impl<'a> Iterator for CommandIter<'a> {
    type Item = Result<LoadCommand<'a>, DyldError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.next_index >= self.command_count {
            return None;
        }

        let index = self.next_index;
        match checked_end(self.cursor, LOAD_COMMAND_HEADER_SIZE) {
            Ok(end) if end <= self.commands_end => {}
            Ok(_) => {
                self.failed = true;
                return Some(Err(DyldError::LoadCommandOutOfBounds { index }));
            }
            Err(error) => {
                self.failed = true;
                return Some(Err(error));
            }
        }

        let command = match read_u32_le(self.image, self.cursor) {
            Ok(value) => value,
            Err(error) => {
                self.failed = true;
                return Some(Err(error));
            }
        };
        let size = match read_u32_le(self.image, self.cursor + 4) {
            Ok(value) => value,
            Err(error) => {
                self.failed = true;
                return Some(Err(error));
            }
        };
        if size < LOAD_COMMAND_HEADER_SIZE as u32 {
            self.failed = true;
            return Some(Err(DyldError::LoadCommandTooSmall { index, size }));
        }

        let end = match checked_end(self.cursor, size as usize) {
            Ok(end) if end <= self.commands_end => end,
            Ok(_) => {
                self.failed = true;
                return Some(Err(DyldError::LoadCommandOutOfBounds { index }));
            }
            Err(error) => {
                self.failed = true;
                return Some(Err(error));
            }
        };
        let bytes = match self.image.get(self.cursor..end) {
            Some(bytes) => bytes,
            None => {
                self.failed = true;
                return Some(Err(DyldError::LoadCommandOutOfBounds { index }));
            }
        };

        self.cursor = end;
        self.next_index += 1;
        Some(Ok(LoadCommand {
            index,
            command,
            bytes,
        }))
    }
}

fn path_from_command<'a>(
    command: LoadCommand<'a>,
    minimum_size: usize,
) -> Result<&'a [u8], DyldError> {
    if command.bytes.len() < minimum_size {
        return Err(DyldError::CommandTooSmall {
            index: command.index,
            command: command.command,
            size: command.bytes.len(),
            minimum: minimum_size,
        });
    }

    let offset = read_u32_le(command.bytes, 8)?;
    if offset < minimum_size as u32 || offset as usize >= command.bytes.len() {
        return Err(DyldError::StringOffsetOutOfBounds {
            index: command.index,
            offset,
            minimum: minimum_size,
            size: command.bytes.len(),
        });
    }

    let tail = &command.bytes[offset as usize..];
    let terminator =
        tail.iter()
            .position(|byte| *byte == 0)
            .ok_or(DyldError::UnterminatedPath {
                index: command.index,
            })?;
    if terminator == 0 {
        return Err(DyldError::EmptyPath {
            index: command.index,
        });
    }
    Ok(&tail[..terminator])
}

fn dylib_kind(command: u32) -> Option<DylibKind> {
    match command {
        LC_LOAD_DYLIB => Some(DylibKind::Required),
        LC_LOAD_WEAK_DYLIB => Some(DylibKind::Weak),
        LC_REEXPORT_DYLIB => Some(DylibKind::Reexport),
        LC_LAZY_LOAD_DYLIB => Some(DylibKind::Lazy),
        LC_LOAD_UPWARD_DYLIB => Some(DylibKind::Upward),
        _ => None,
    }
}

fn parse_dylib(command: LoadCommand<'_>, kind: DylibKind) -> Result<Dylib<'_>, DyldError> {
    let path = path_from_command(command, DYLIB_COMMAND_SIZE)?;
    Ok(Dylib {
        command_index: command.index,
        kind,
        path,
        timestamp: read_u32_le(command.bytes, 12)?,
        current_version: read_u32_le(command.bytes, 16)?,
        compatibility_version: read_u32_le(command.bytes, 20)?,
    })
}

pub fn dylibs(bytes: &[u8], slice: MachSlice) -> Result<DylibIter<'_>, DyldError> {
    Ok(DylibIter {
        commands: commands(bytes, slice)?,
        failed: false,
    })
}

impl<'a> Iterator for DylibIter<'a> {
    type Item = Result<Dylib<'a>, DyldError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }

        for command in self.commands.by_ref() {
            let command = match command {
                Ok(command) => command,
                Err(error) => {
                    self.failed = true;
                    return Some(Err(error));
                }
            };
            let Some(kind) = dylib_kind(command.command) else {
                continue;
            };
            return Some(match parse_dylib(command, kind) {
                Ok(dylib) => Ok(dylib),
                Err(error) => {
                    self.failed = true;
                    Err(error)
                }
            });
        }
        None
    }
}

pub fn run_paths(bytes: &[u8], slice: MachSlice) -> Result<RunPathIter<'_>, DyldError> {
    Ok(RunPathIter {
        commands: commands(bytes, slice)?,
        failed: false,
    })
}

impl<'a> Iterator for RunPathIter<'a> {
    type Item = Result<RunPath<'a>, DyldError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }

        for command in self.commands.by_ref() {
            let command = match command {
                Ok(command) => command,
                Err(error) => {
                    self.failed = true;
                    return Some(Err(error));
                }
            };
            if command.command != LC_RPATH {
                continue;
            }
            return Some(match path_from_command(command, RPATH_COMMAND_SIZE) {
                Ok(path) => Ok(RunPath {
                    command_index: command.index,
                    path,
                }),
                Err(error) => {
                    self.failed = true;
                    Err(error)
                }
            });
        }
        None
    }
}

pub fn dynamic_linker(bytes: &[u8], slice: MachSlice) -> Result<Option<&[u8]>, DyldError> {
    let mut linker = None;
    for command in commands(bytes, slice)? {
        let command = command?;
        if command.command != LC_LOAD_DYLINKER {
            continue;
        }
        if linker.is_some() {
            return Err(DyldError::DuplicateDynamicLinker);
        }
        linker = Some(path_from_command(command, DYLINKER_COMMAND_SIZE)?);
    }
    Ok(linker)
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use aw_macho::{CpuType, select_mach_o_64};
    use std::vec;
    use std::vec::Vec;

    const MH_MAGIC_64: u32 = 0xfeed_facf;
    const MH_EXECUTE: u32 = 0x2;
    const CPU_TYPE_X86_64: u32 = 0x0100_0007;

    fn set_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn aligned_command_size(size: usize) -> usize {
        (size + 7) & !7
    }

    fn dylib_command(command: u32, path: &[u8], version: u32) -> Vec<u8> {
        let size = aligned_command_size(DYLIB_COMMAND_SIZE + path.len() + 1);
        let mut out = vec![0u8; size];
        set_u32(&mut out, 0, command);
        set_u32(&mut out, 4, size as u32);
        set_u32(&mut out, 8, DYLIB_COMMAND_SIZE as u32);
        set_u32(&mut out, 12, 1);
        set_u32(&mut out, 16, version);
        set_u32(&mut out, 20, 0x0001_0000);
        out[DYLIB_COMMAND_SIZE..DYLIB_COMMAND_SIZE + path.len()].copy_from_slice(path);
        out
    }

    fn simple_path_command(command: u32, minimum: usize, path: &[u8]) -> Vec<u8> {
        let size = aligned_command_size(minimum + path.len() + 1);
        let mut out = vec![0u8; size];
        set_u32(&mut out, 0, command);
        set_u32(&mut out, 4, size as u32);
        set_u32(&mut out, 8, minimum as u32);
        out[minimum..minimum + path.len()].copy_from_slice(path);
        out
    }

    fn fixture(command_list: &[Vec<u8>]) -> Vec<u8> {
        let command_bytes: usize = command_list.iter().map(Vec::len).sum();
        let mut out = Vec::with_capacity(MACH_HEADER_64_SIZE + command_bytes);
        out.extend_from_slice(&MH_MAGIC_64.to_le_bytes());
        out.extend_from_slice(&CPU_TYPE_X86_64.to_le_bytes());
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(&MH_EXECUTE.to_le_bytes());
        out.extend_from_slice(&(command_list.len() as u32).to_le_bytes());
        out.extend_from_slice(&(command_bytes as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        for command in command_list {
            out.extend_from_slice(command);
        }
        out
    }

    fn selected(image: &[u8]) -> MachSlice {
        select_mach_o_64(image, CpuType::X86_64).unwrap()
    }

    #[test]
    fn parses_dynamic_linker_path() {
        let image = fixture(&[simple_path_command(
            LC_LOAD_DYLINKER,
            DYLINKER_COMMAND_SIZE,
            b"/usr/lib/dyld",
        )]);
        assert_eq!(
            dynamic_linker(&image, selected(&image)).unwrap(),
            Some(&b"/usr/lib/dyld"[..])
        );
    }

    #[test]
    fn parses_all_supported_dylib_kinds() {
        let image = fixture(&[
            dylib_command(LC_LOAD_DYLIB, b"/usr/lib/libSystem.B.dylib", 1),
            dylib_command(LC_LOAD_WEAK_DYLIB, b"@rpath/Weak.dylib", 2),
            dylib_command(LC_REEXPORT_DYLIB, b"@rpath/Reexport.dylib", 3),
            dylib_command(LC_LAZY_LOAD_DYLIB, b"@rpath/Lazy.dylib", 4),
            dylib_command(LC_LOAD_UPWARD_DYLIB, b"@rpath/Upward.dylib", 5),
        ]);
        let parsed: Vec<_> = dylibs(&image, selected(&image))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(parsed.len(), 5);
        assert_eq!(parsed[0].kind, DylibKind::Required);
        assert_eq!(parsed[1].kind, DylibKind::Weak);
        assert_eq!(parsed[2].kind, DylibKind::Reexport);
        assert_eq!(parsed[3].kind, DylibKind::Lazy);
        assert_eq!(parsed[4].kind, DylibKind::Upward);
        assert_eq!(parsed[4].current_version, 5);
    }

    #[test]
    fn parses_run_paths() {
        let image = fixture(&[
            simple_path_command(LC_RPATH, RPATH_COMMAND_SIZE, b"@loader_path/Frameworks"),
            simple_path_command(LC_RPATH, RPATH_COMMAND_SIZE, b"/opt/aw/lib"),
        ]);
        let paths: Vec<_> = run_paths(&image, selected(&image))
            .unwrap()
            .map(|path| path.unwrap().path)
            .collect();
        assert_eq!(
            paths,
            vec![&b"@loader_path/Frameworks"[..], &b"/opt/aw/lib"[..]]
        );
    }

    #[test]
    fn rejects_duplicate_dynamic_linker_commands() {
        let image = fixture(&[
            simple_path_command(LC_LOAD_DYLINKER, DYLINKER_COMMAND_SIZE, b"/usr/lib/dyld"),
            simple_path_command(LC_LOAD_DYLINKER, DYLINKER_COMMAND_SIZE, b"/other/dyld"),
        ]);
        assert_eq!(
            dynamic_linker(&image, selected(&image)),
            Err(DyldError::DuplicateDynamicLinker)
        );
    }

    #[test]
    fn rejects_string_offset_inside_fixed_command_fields() {
        let mut command = simple_path_command(LC_RPATH, RPATH_COMMAND_SIZE, b"/valid");
        set_u32(&mut command, 8, 8);
        let image = fixture(&[command]);
        let mut paths = run_paths(&image, selected(&image)).unwrap();
        assert_eq!(
            paths.next(),
            Some(Err(DyldError::StringOffsetOutOfBounds {
                index: 0,
                offset: 8,
                minimum: RPATH_COMMAND_SIZE,
                size: 24,
            }))
        );
    }

    #[test]
    fn rejects_unterminated_path() {
        let mut command = vec![0u8; 16];
        set_u32(&mut command, 0, LC_LOAD_DYLINKER);
        set_u32(&mut command, 4, 16);
        set_u32(&mut command, 8, DYLINKER_COMMAND_SIZE as u32);
        command[12..16].copy_from_slice(b"dyld");
        let image = fixture(&[command]);
        assert_eq!(
            dynamic_linker(&image, selected(&image)),
            Err(DyldError::UnterminatedPath { index: 0 })
        );
    }

    #[test]
    fn rejects_empty_path() {
        let image = fixture(&[simple_path_command(
            LC_LOAD_DYLINKER,
            DYLINKER_COMMAND_SIZE,
            b"",
        )]);
        assert_eq!(
            dynamic_linker(&image, selected(&image)),
            Err(DyldError::EmptyPath { index: 0 })
        );
    }
}
