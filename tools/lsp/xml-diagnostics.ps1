# Validate a source snapshot with the installed .NET XML/XSD implementation.
# No document code executes, no sources change and external resolution is
# restricted to files inside the applicable workspace.
#requires -Version 7.4
$ErrorActionPreference = 'Stop'
[Console]::InputEncoding = [Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$payload = [Console]::In.ReadToEnd() | ConvertFrom-Json
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Net;
using System.Xml;
public sealed class HarnessXmlResolver : XmlUrlResolver {
    public string Root;
    public string Rejected;
    public override object GetEntity(Uri uri, string role, Type objectType) {
        var path = uri.IsFile ? Path.GetFullPath(uri.LocalPath) : "";
        var boundary = Path.GetFullPath(Root).TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
        if (!uri.IsFile || !path.StartsWith(boundary, StringComparison.OrdinalIgnoreCase)) {
            Rejected = "XML schema or entity requires an unavailable external root: " + uri;
            throw new XmlException(Rejected);
        }
        return base.GetEntity(uri, role, objectType);
    }
}
'@
$resolver = [HarnessXmlResolver]::new()
$resolver.Root = $payload.workspace
$settings = [Xml.XmlReaderSettings]::new()
$settings.XmlResolver = $resolver
$settings.DtdProcessing = [Xml.DtdProcessing]::Parse
$settings.MaxCharactersFromEntities = 1000000
$settings.MaxCharactersInDocument = 8388608
$items = [Collections.Generic.List[object]]::new()
function Add-XmlIssue($Issue, [int]$Severity, [string]$Code) {
    $line = [Math]::Max(0, $Issue.LineNumber - 1)
    $column = [Math]::Max(0, $Issue.LinePosition - 1)
    $items.Add(@{range=@{start=@{line=$line;character=$column};end=@{line=$line;character=$column+1}};severity=$Severity;source='System.Xml';code=$Code;message=$Issue.Message})
}
if ($payload.text -match 'schemaLocation\s*=' -or $payload.schemas.Count -gt 0) {
    $settings.ValidationType = [Xml.ValidationType]::Schema
    $settings.ValidationFlags = [Xml.Schema.XmlSchemaValidationFlags]::ProcessSchemaLocation -bor [Xml.Schema.XmlSchemaValidationFlags]::ProcessInlineSchema
    $settings.Schemas.XmlResolver = $resolver
    foreach ($schema in $payload.schemas) { $null = $settings.Schemas.Add($null, [string]$schema) }
    $settings.add_ValidationEventHandler({param($sender,$eventArgs) Add-XmlIssue $eventArgs.Exception $(if ($eventArgs.Severity -eq 'Error') {1} else {2}) 'XMLSchema'})
}
$reader = $null
try {
    $reader = [Xml.XmlReader]::Create([IO.StringReader]::new($payload.text), $settings, [string]$payload.uri)
    while ($reader.Read()) { }
} catch [Xml.XmlException] {
    Add-XmlIssue $_.Exception 1 'XMLSyntax'
} finally {
    if ($reader) { $reader.Dispose() }
}
@{complete=(-not [bool]$resolver.Rejected);reason=$resolver.Rejected;diagnostics=@($items)} | ConvertTo-Json -Depth 10 -Compress
