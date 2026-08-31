#!/usr/bin/env python3
"""Validate OpDev's Codex plugin distribution contract without host state."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path, PurePosixPath
from typing import Any


SEMVER = re.compile(
    r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)"
    r"(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)
PLUGIN_KEYS = {
    "id",
    "name",
    "version",
    "description",
    "skills",
    "apps",
    "mcpServers",
    "interface",
    "author",
    "homepage",
    "repository",
    "license",
    "keywords",
}
INTERFACE_KEYS = {
    "displayName",
    "shortDescription",
    "longDescription",
    "developerName",
    "category",
    "capabilities",
    "websiteURL",
    "privacyPolicyURL",
    "termsOfServiceURL",
    "brandColor",
    "composerIcon",
    "logo",
    "logoDark",
    "screenshots",
    "defaultPrompt",
    "default_prompt",
}


def load_object(path: Path, errors: list[str]) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        errors.append(f"{path} is not readable JSON: {error}")
        return {}
    if not isinstance(value, dict):
        errors.append(f"{path} must contain a JSON object")
        return {}
    return value


def require_string(value: dict[str, Any], key: str, label: str, errors: list[str]) -> str:
    result = value.get(key)
    if not isinstance(result, str) or not result.strip():
        errors.append(f"{label}.{key} must be a non-empty string")
        return ""
    return result


def check_relative_file(plugin_root: Path, raw: Any, label: str, errors: list[str]) -> None:
    if not isinstance(raw, str) or not raw.strip():
        errors.append(f"{label} must be a non-empty relative path")
        return
    candidate = PurePosixPath(raw.replace("\\", "/"))
    if candidate.is_absolute() or ".." in candidate.parts:
        errors.append(f"{label} must stay inside the plugin")
        return
    resolved = (plugin_root / candidate.as_posix()).resolve()
    if not resolved.is_relative_to(plugin_root.resolve()) or not resolved.is_file():
        errors.append(f"{label} points to a missing file inside the plugin")


def validate(plugin_root: Path) -> list[str]:
    errors: list[str] = []
    manifest_path = plugin_root / ".codex-plugin" / "plugin.json"
    manifest = load_object(manifest_path, errors)
    if not manifest:
        return errors

    unknown = sorted(set(manifest) - PLUGIN_KEYS)
    if unknown:
        errors.append(f"plugin.json has unsupported fields: {', '.join(unknown)}")
    if "[TODO:" in json.dumps(manifest):
        errors.append("plugin.json contains an unresolved TODO placeholder")

    name = require_string(manifest, "name", "plugin", errors)
    version = require_string(manifest, "version", "plugin", errors)
    require_string(manifest, "description", "plugin", errors)
    if name != plugin_root.name:
        errors.append("plugin name must match its directory name")
    if version and not SEMVER.fullmatch(version):
        errors.append("plugin.version must be strict semantic versioning")

    author = manifest.get("author")
    if not isinstance(author, dict):
        errors.append("plugin.author must be an object")
    else:
        require_string(author, "name", "plugin.author", errors)
        unknown_author = sorted(set(author) - {"name", "email", "url"})
        if unknown_author:
            errors.append(f"plugin.author has unsupported fields: {', '.join(unknown_author)}")

    if manifest.get("skills") not in (None, "./skills/"):
        errors.append("plugin.skills must resolve to ./skills/")

    interface = manifest.get("interface")
    if not isinstance(interface, dict):
        errors.append("plugin.interface must be an object")
    else:
        unknown_interface = sorted(set(interface) - INTERFACE_KEYS)
        if unknown_interface:
            errors.append(
                f"plugin.interface has unsupported fields: {', '.join(unknown_interface)}"
            )
        for key in (
            "displayName",
            "shortDescription",
            "longDescription",
            "developerName",
            "category",
        ):
            require_string(interface, key, "plugin.interface", errors)
        capabilities = interface.get("capabilities")
        if not isinstance(capabilities, list) or not capabilities or not all(
            isinstance(item, str) and item.strip() for item in capabilities
        ):
            errors.append("plugin.interface.capabilities must be a non-empty string array")
        if "defaultPrompt" not in interface and "default_prompt" not in interface:
            errors.append("plugin.interface must declare a default prompt")
        for key in ("composerIcon", "logo", "logoDark"):
            if key in interface:
                check_relative_file(plugin_root, interface[key], f"plugin.interface.{key}", errors)
        screenshots = interface.get("screenshots", [])
        if not isinstance(screenshots, list):
            errors.append("plugin.interface.screenshots must be an array")
        else:
            for index, screenshot in enumerate(screenshots):
                check_relative_file(
                    plugin_root,
                    screenshot,
                    f"plugin.interface.screenshots[{index}]",
                    errors,
                )

    skills_root = plugin_root / "skills"
    if not skills_root.is_dir():
        errors.append("plugin must contain a skills directory")
    else:
        for skill_root in sorted(path for path in skills_root.iterdir() if path.is_dir()):
            skill_path = skill_root / "SKILL.md"
            try:
                contents = skill_path.read_text(encoding="utf-8")
            except OSError as error:
                errors.append(f"{skill_path} is not readable: {error}")
                continue
            if not contents.startswith("---\n") or "\n---\n" not in contents[4:]:
                errors.append(f"{skill_path} must contain closed YAML frontmatter")
                continue
            frontmatter = contents[4 : contents.find("\n---\n", 4)]
            if not re.search(r"(?m)^name:\s*\S+", frontmatter):
                errors.append(f"{skill_path} frontmatter must declare name")
            if not re.search(r"(?m)^description:\s*\S+", frontmatter):
                errors.append(f"{skill_path} frontmatter must declare description")

    compatibility = load_object(plugin_root / "opdev-compatibility.json", errors)
    if compatibility:
        if set(compatibility) != {"schema", "plugin", "requires"}:
            errors.append("compatibility contract must contain only schema, plugin, and requires")
        if compatibility.get("schema") != 1:
            errors.append("compatibility contract schema must be 1")
        identity = compatibility.get("plugin")
        requirements = compatibility.get("requires")
        if not isinstance(identity, dict) or set(identity) != {"name", "version"}:
            errors.append("compatibility.plugin must contain only name and version")
        elif identity.get("name") != name or identity.get("version") != version:
            errors.append("compatibility plugin identity must match plugin.json")
        if not isinstance(requirements, dict) or set(requirements) != {"cli"}:
            errors.append("compatibility.requires must contain only cli")
        elif not isinstance(requirements.get("cli"), str) or not requirements["cli"].strip():
            errors.append("compatibility.requires.cli must be a non-empty SemVer range")

    repository_root = plugin_root.parent.parent
    marketplace = load_object(repository_root / ".agents" / "plugins" / "marketplace.json", errors)
    if marketplace:
        entries = marketplace.get("plugins")
        matching = (
            [entry for entry in entries if isinstance(entry, dict) and entry.get("name") == name]
            if isinstance(entries, list)
            else []
        )
        if len(matching) != 1:
            errors.append("Codex marketplace must contain exactly one matching plugin entry")
        else:
            entry = matching[0]
            if entry.get("source") != {"source": "local", "path": f"./plugins/{name}"}:
                errors.append("Codex marketplace plugin source is not canonical")
            if not isinstance(entry.get("policy"), dict) or "category" not in entry:
                errors.append("Codex marketplace entry must declare policy and category")

    return errors


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("plugin_path", type=Path)
    args = parser.parse_args()
    plugin_root = args.plugin_path.resolve()
    errors = validate(plugin_root)
    if errors:
        print("Codex plugin validation failed:")
        for error in errors:
            print(f"- {error}")
        raise SystemExit(1)
    print(f"Codex plugin validation passed: {plugin_root}")


if __name__ == "__main__":
    main()
