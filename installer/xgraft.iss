#define MyAppVersion GetEnv("APP_VERSION")

[Setup]
AppName=xgraft
AppVersion={#MyAppVersion}
DefaultDirName={localappdata}\xgraft
DefaultGroupName=xgraft
OutputDir=output
OutputBaseFilename=xgraft-setup-{#MyAppVersion}
Compression=lzma2
SolidCompression=yes
UninstallDisplayIcon={app}\xgraft.exe

[Files]
Source: "..\target\x86_64-pc-windows-msvc\release\xgraft.exe"; DestDir: "{app}"

[Code]

procedure EnvAddPath(Path: string);
var
    Paths: string;
begin
    if not RegQueryStringValue(
        HKEY_CURRENT_USER,
        'Environment',
        'Path',
        Paths
    ) then
        Paths := '';

    if Pos(';' + Uppercase(Path) + ';', ';' + Uppercase(Paths) + ';') = 0 then
    begin
        Paths := Paths + ';' + Path;

        RegWriteStringValue(
            HKEY_CURRENT_USER,
            'Environment',
            'Path',
            Paths
        );
    end;
end;

procedure EnvRemovePath(Path: string);
var
    Paths: string;
    P: Integer;
begin
    if RegQueryStringValue(
        HKEY_CURRENT_USER,
        'Environment',
        'Path',
        Paths
    ) then
    begin
        P := Pos(';' + Uppercase(Path), ';' + Uppercase(Paths));

        if P > 0 then
        begin
            Delete(Paths, P, Length(Path) + 1);

            RegWriteStringValue(
                HKEY_CURRENT_USER,
                'Environment',
                'Path',
                Paths
            );
        end;
    end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
    if CurStep = ssPostInstall then
        EnvAddPath(ExpandConstant('{app}'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
    if CurUninstallStep = usPostUninstall then
        EnvRemovePath(ExpandConstant('{app}'));
end;