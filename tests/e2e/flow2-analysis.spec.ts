import { test, expect } from "@playwright/test";
import * as fs from "fs";
import * as path from "path";
import * as os from "os";
import { execSync } from "child_process";

test.describe("Flow 2 - Code Analysis (Phase 2)", () => {
  let tempRepoPath: string;

  test.beforeAll(() => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "cid-e2e-analysis-"));
    tempRepoPath = path.join(tmpDir, "analysis-repo");
    fs.mkdirSync(tempRepoPath);

    execSync("git init", { cwd: tempRepoPath });
    execSync('git config user.email "e2e@cid.test"', { cwd: tempRepoPath });
    execSync('git config user.name "CID E2E"', { cwd: tempRepoPath });

    // Create multi-language sample files
    fs.writeFileSync(path.join(tempRepoPath, "main.rs"), `
fn main() {
    println!("Hello, analysis!");
}

fn add(a: i32, b: i32) -> i32 {
    a + b
}

struct Config {
    name: String,
    port: u16,
}

impl Config {
    fn new(name: &str) -> Self {
        Config { name: name.to_string(), port: 8080 }
    }
}
`);

    fs.writeFileSync(path.join(tempRepoPath, "app.ts"), `
import { Component } from './component';
import { HttpClient } from '@angular/core/http';

export class AppComponent {
    title = 'cid-analysis';
    
    constructor(private http: HttpClient) {}
    
    async loadData(): Promise<void> {
        const data = await this.http.get('/api/data').toPromise();
        console.log(data);
    }
}
`);

    fs.writeFileSync(path.join(tempRepoPath, "utils.py"), `
import os
import sys
from typing import List, Optional

class ConfigManager:
    def __init__(self, config_path: str):
        self.config_path = config_path
        self.settings = {}
    
    def load(self) -> dict:
        with open(self.config_path) as f:
            return json.load(f)
    
    def save(self, settings: dict) -> None:
        with open(self.config_path, 'w') as f:
            json.dump(settings, f)

def calculate_total(items: List[float]) -> float:
    return sum(items)
`);

    fs.writeFileSync(path.join(tempRepoPath, "server.go"), `
package main

import (
    "fmt"
    "net/http"
)

type Server struct {
    Port int
    Name string
}

func (s *Server) Start() error {
    return http.ListenAndServe(fmt.Sprintf(":%d", s.Port), nil)
}

func NewServer(port int) *Server {
    return &Server{Port: port, Name: "CID Analysis Server"}
}
`);

    execSync("git add .", { cwd: tempRepoPath });
    execSync('git commit -m "initial commit with multi-language sample"', { cwd: tempRepoPath });

    console.log(`[E2E] Created analysis test repo at ${tempRepoPath}`);
  });

  test.afterAll(() => {
    if (tempRepoPath) {
      try {
        fs.rmSync(path.dirname(tempRepoPath), { recursive: true, force: true });
      } catch {
        // best-effort cleanup; a leftover temp dir doesn't fail the suite
      }
    }
  });

  test("should analyze Rust file via API", async () => {
    const rustFile = path.join(tempRepoPath, "main.rs");
    const resp = await fetch("http://127.0.0.1:5919/api/rpc", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: "1",
        method: "code.analyze_file",
        params: { file_path: rustFile },
      }),
    }).catch(() => null);

    if (!resp) {
      test.skip(true, "Core not running at http://127.0.0.1:5919");
      return;
    }

    const data = await resp.json();
    expect(data.result).toBeDefined();
    expect(data.result.language).toBe("rust");
    expect(data.result.symbols.length).toBeGreaterThanOrEqual(3);

    const functionNames = data.result.symbols.map((s) => s.name);
    expect(functionNames).toContain("main");
    expect(functionNames).toContain("add");

    const structNames = data.result.symbols.filter((s) => s.kind === "struct").map((s) => s.name);
    expect(structNames).toContain("Config");

    console.log(`[E2E] Found symbols: ${JSON.stringify(functionNames)}`);
  });

  test("should analyze TypeScript file via API", async () => {
    const tsFile = path.join(tempRepoPath, "app.ts");
    const resp = await fetch("http://127.0.0.1:5919/api/rpc", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: "2",
        method: "code.analyze_file",
        params: { file_path: tsFile },
      }),
    }).catch(() => null);

    if (!resp) {
      test.skip(true, "Core not running");
      return;
    }

    const data = await resp.json();
    expect(data.result).toBeDefined();
    expect(data.result.imports.length).toBeGreaterThan(0);
    expect(data.result.imports.some((i: string) => i.includes("./component"))).toBeTruthy();
  });

  test("should analyze entire directory via API", async () => {
    const resp = await fetch("http://127.0.0.1:5919/api/rpc", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: "3",
        method: "code.analyze_directory",
        params: { dir_path: tempRepoPath },
      }),
    }).catch(() => null);

    if (!resp) {
      test.skip(true, "Core not running");
      return;
    }

    const data = await resp.json();
    expect(data.result).toBeDefined();
    expect(data.result.length).toBeGreaterThanOrEqual(4);

    const languages = data.result.map((f) => f.language);
    expect(languages).toContain("rust");
    expect(languages).toContain("typescript");
    expect(languages).toContain("python");
    expect(languages).toContain("go");

    console.log(`[E2E] Analyzed ${data.result.length} files across ${new Set(languages).size} languages`);
  });

  test("should search symbols by name", async () => {
    const resp = await fetch("http://127.0.0.1:5919/api/rpc", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: "4",
        method: "code.search_symbols",
        params: { dir_path: tempRepoPath, query: "Config" },
      }),
    }).catch(() => null);

    if (!resp) {
      test.skip(true, "Core not running");
      return;
    }

    const data = await resp.json();
    expect(data.result).toBeDefined();
    expect(data.result.total).toBeGreaterThanOrEqual(2); // Rust Config struct + Python ConfigManager class
    console.log(`[E2E] Found ${data.result.total} symbols matching 'Config'`);
  });

  test("should get imports from TypeScript file", async () => {
    const tsFile = path.join(tempRepoPath, "app.ts");
    const resp = await fetch("http://127.0.0.1:5919/api/rpc", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: "5",
        method: "code.get_imports",
        params: { file_path: tsFile },
      }),
    }).catch(() => null);

    if (!resp) {
      test.skip(true, "Core not running");
      return;
    }

    const data = await resp.json();
    expect(data.result).toBeDefined();
    expect(data.result.imports.length).toBeGreaterThanOrEqual(2);
    console.log(`[E2E] Imports: ${JSON.stringify(data.result.imports)}`);
  });
});
