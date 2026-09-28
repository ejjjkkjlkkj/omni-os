from __future__ import annotations
from dataclasses import dataclass
import pathlib
import struct

EFI_HII_PACKAGE_FORMS = 0x02
EFI_IFR_END_OP = 0x29

OPCODES = {
    0x01: "FORM",
    0x02: "SUBTITLE",
    0x03: "TEXT",
    0x04: "IMAGE",
    0x05: "ONE_OF",
    0x06: "CHECKBOX",
    0x07: "NUMERIC",
    0x08: "PASSWORD",
    0x09: "ONE_OF_OPTION",
    0x0A: "SUPPRESS_IF",
    0x0B: "LOCKED",
    0x0C: "ACTION",
    0x0D: "RESET_BUTTON",
    0x0E: "FORM_SET",
    0x0F: "REF",
    0x10: "NO_SUBMIT_IF",
    0x11: "INCONSISTENT_IF",
    0x18: "RULE",
    0x19: "GRAY_OUT_IF",
    0x1A: "DATE",
    0x1B: "TIME",
    0x1C: "STRING",
    0x1D: "REFRESH",
    0x1E: "DISABLE_IF",
    0x23: "ORDERED_LIST",
    0x24: "VARSTORE",
    0x25: "VARSTORE_NAME_VALUE",
    0x26: "VARSTORE_EFI",
    0x27: "VARSTORE_DEVICE",
    0x28: "VERSION",
    0x29: "END",
    0x5B: "DEFAULT",
    0x5C: "DEFAULTSTORE",
    0x5D: "FORM_MAP",
    0x5F: "GUID",
    0x60: "SECURITY",
    0x61: "MODAL_TAG",
    0x62: "REFRESH_ID",
    0x63: "WARNING_IF",
    0x64: "MATCH2",
}

ROLE_BY_OPCODE = {
    0x01: "form",
    0x02: "text",
    0x03: "text",
    0x05: "radiogroup",
    0x06: "checkbox",
    0x07: "spinbutton",
    0x08: "edit",
    0x0C: "button",
    0x0D: "button",
    0x0E: "formset",
    0x0F: "link",
    0x1A: "edit",
    0x1B: "edit",
    0x1C: "edit",
    0x23: "list",
}

QUESTION_OPS = {0x05, 0x06, 0x07, 0x08, 0x0C, 0x0F, 0x1A, 0x1B, 0x1C, 0x23}
STATEMENT_OPS = QUESTION_OPS | {0x02, 0x03, 0x0D}

class IfrError(ValueError):
    pass

@dataclass(frozen=True, slots=True)
class IfrOp:
    offset: int
    opcode: int
    name: str
    length: int
    scope: bool
    depth: int
    role: str | None
    prompt_id: int | None = None
    help_id: int | None = None
    question_id: int | None = None
    password: bool = False

    def as_dict(self) -> dict[str, object]:
        return {
            "offset": self.offset,
            "opcode": self.opcode,
            "name": self.name,
            "length": self.length,
            "scope": self.scope,
            "depth": self.depth,
            "role": self.role,
            "prompt_id": self.prompt_id,
            "help_id": self.help_id,
            "question_id": self.question_id,
            "password": self.password,
        }

def _u16(data: bytes, offset: int) -> int:
    return struct.unpack_from("<H", data, offset)[0]

def parse_ifr_stream(data: bytes) -> list[IfrOp]:
    ops: list[IfrOp] = []
    offset = 0
    depth = 0

    while offset < len(data):
        if len(data) - offset < 2:
            raise IfrError(f"truncated IFR header at {offset}")

        opcode = data[offset]
        encoded = data[offset + 1]
        length = encoded & 0x7F
        scope = bool(encoded & 0x80)

        if length < 2:
            raise IfrError(f"invalid IFR length {length} at {offset}")
        end = offset + length
        if end > len(data):
            raise IfrError(f"IFR record at {offset} exceeds stream")

        if opcode == EFI_IFR_END_OP:
            if scope:
                raise IfrError(f"END opcode must not open a scope at {offset}")
            if depth == 0:
                raise IfrError(f"scope underflow at {offset}")
            depth -= 1
            op_depth = depth
        else:
            op_depth = depth

        prompt_id = help_id = question_id = None
        if opcode in STATEMENT_OPS:
            if length < 6:
                raise IfrError(f"statement opcode {opcode:#x} too short at {offset}")
            prompt_id = _u16(data, offset + 2)
            help_id = _u16(data, offset + 4)

        if opcode in QUESTION_OPS:
            if length < 13:
                raise IfrError(f"question opcode {opcode:#x} too short at {offset}")
            question_id = _u16(data, offset + 6)

        ops.append(IfrOp(
            offset=offset,
            opcode=opcode,
            name=OPCODES.get(opcode, f"OP_{opcode:02X}"),
            length=length,
            scope=scope,
            depth=op_depth,
            role=ROLE_BY_OPCODE.get(opcode),
            prompt_id=prompt_id,
            help_id=help_id,
            question_id=question_id,
            password=(opcode == 0x08),
        ))

        if opcode != EFI_IFR_END_OP and scope:
            depth += 1

        offset = end

    if depth != 0:
        raise IfrError(f"unclosed IFR scopes: {depth}")

    return ops

def parse_hii_package_list(data: bytes) -> dict[str, object]:
    if len(data) < 20:
        raise IfrError("HII package-list header truncated")

    declared_length = struct.unpack_from("<I", data, 16)[0]
    if declared_length < 20 or declared_length > len(data):
        raise IfrError("invalid HII package-list length")

    packages: list[dict[str, object]] = []
    offset = 20
    while offset < declared_length:
        if declared_length - offset < 4:
            raise IfrError(f"truncated HII package header at {offset}")
        header = struct.unpack_from("<I", data, offset)[0]
        length = header & 0x00FFFFFF
        package_type = (header >> 24) & 0xFF
        if length < 4:
            raise IfrError(f"invalid HII package length {length} at {offset}")
        end = offset + length
        if end > declared_length:
            raise IfrError(f"HII package at {offset} exceeds package list")

        item: dict[str, object] = {
            "offset": offset,
            "length": length,
            "type": package_type,
        }
        if package_type == EFI_HII_PACKAGE_FORMS:
            item["ifr"] = [op.as_dict() for op in parse_ifr_stream(data[offset + 4:end])]
        packages.append(item)
        offset = end

    if offset != declared_length:
        raise IfrError("HII package list did not terminate exactly")

    return {
        "declared_length": declared_length,
        "packages": packages,
    }

def inspect_file(path: str | pathlib.Path) -> dict[str, object]:
    source = pathlib.Path(path)
    result = parse_hii_package_list(source.read_bytes())
    return {"path": str(source), **result, "mutation": False}
