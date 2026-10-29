#include "MappedLocalFile.h"
#include <QCryptographicHash>
#include <QDir>
#include <QElapsedTimer>
#include <QRegularExpression>
#include <QStringList>
#ifdef Q_OS_WIN
#include <qt_windows.h>
#include <vector>
#endif

namespace MappedLocalFile {
struct Guard {
#ifdef Q_OS_WIN
    std::vector<HANDLE> handles;
    ~Guard() { for (auto i=handles.rbegin();i!=handles.rend();++i) CloseHandle(*i); }
#endif
};
bool safeDrivePath(const QString &path)
{
    // Deliberately excludes UNC/device namespaces, ADS, shell aliases, empty
    // components, control characters and Win32 trailing-dot/space rewriting.
    if (path.size()>8192 || !QRegularExpression("^[A-Za-z]:[/\\\\]").match(path).hasMatch()) return false;
    const auto rest=QString(path.mid(3)).replace('\\','/');
    static const QRegularExpression reserved("^(CON|PRN|AUX|NUL|CONIN\\$|CONOUT\\$|COM[0-9\u00b9\u00b2\u00b3]|LPT[0-9\u00b9\u00b2\u00b3])(?:[.]|$)",QRegularExpression::CaseInsensitiveOption);
    for (const auto &part:rest.split('/')) {
        if (part.isEmpty() || part=="." || part==".." || part.endsWith('.') || part.endsWith(' ') || reserved.match(part).hasMatch()) return false;
        for (const auto ch:part) if (ch.isNull() || ch.category()==QChar::Other_Control || QStringLiteral("<>:\"|?*\\").contains(ch)) return false;
    }
    return true;
}
Result verify(const QString &path,const QString &sha256,qint64 bytes)
{
    Result result;
    if (!safeDrivePath(path) || !QRegularExpression("^[0-9a-f]{64}$").match(sha256).hasMatch() || bytes<1 || bytes>10*1024*1024) {
        result.error=QStringLiteral("The mapped local path or file evidence is invalid.");return result;
    }
#ifndef Q_OS_WIN
    result.error=QStringLiteral("Opening a mapped Windows original requires a Windows desktop.");return result;
#else
    auto guard=std::make_shared<Guard>();
    const auto clean=QDir::fromNativeSeparators(path);
    const auto root=QDir::toNativeSeparators(clean.left(3)).toStdWString();
    if(GetDriveTypeW(root.c_str())!=DRIVE_FIXED){result.error=QStringLiteral("Only a local fixed-drive workspace can be opened through this mapping.");return result;}
    QStringList prefixes{clean.left(3)};
    for(int i=3;i<clean.size();++i)if(clean[i]=='/')prefixes<<clean.left(i);
    prefixes<<clean;
    QElapsedTimer elapsed;elapsed.start();
    for(qsizetype i=0;i<prefixes.size();++i){
        const bool leaf=i+1==prefixes.size();
        const auto native=(QStringLiteral("\\\\?\\")+QDir::toNativeSeparators(prefixes[i])).toStdWString();
        const HANDLE handle=CreateFileW(native.c_str(),leaf?GENERIC_READ:FILE_READ_ATTRIBUTES,
            leaf?FILE_SHARE_READ:FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT|(leaf?FILE_ATTRIBUTE_NORMAL:FILE_FLAG_BACKUP_SEMANTICS),nullptr);
        if(handle==INVALID_HANDLE_VALUE){result.error=QStringLiteral("The local file is unavailable, busy or unreadable. Nothing was opened.");return result;}
        guard->handles.push_back(handle);
        BY_HANDLE_FILE_INFORMATION info{};
        if(!GetFileInformationByHandle(handle,&info)||GetFileType(handle)!=FILE_TYPE_DISK
            ||(info.dwFileAttributes&FILE_ATTRIBUTE_REPARSE_POINT)
            ||bool(info.dwFileAttributes&FILE_ATTRIBUTE_DIRECTORY)==leaf){
            result.error=QStringLiteral("The mapped path contains a reparse point or a non-regular file. Use the existing download workflow instead.");return result;
        }
        std::vector<wchar_t> actual(32768);
        const DWORD size=GetFinalPathNameByHandleW(handle,actual.data(),DWORD(actual.size()),FILE_NAME_NORMALIZED|VOLUME_NAME_DOS);
        if(!size||size>=actual.size()){result.error=QStringLiteral("The final local file identity could not be verified.");return result;}
        // Preserve the Win32 namespace until it has been checked: Qt's
        // native-path conversion can consume an extended-length prefix.
        auto final=QString::fromWCharArray(actual.data(),int(size)).replace('\\','/');
        if(!final.startsWith("//?/")){result.error=QStringLiteral("The local path resolved outside the selected drive namespace.");return result;}
        final.remove(0,4);
        if(final.compare(prefixes[i],Qt::CaseInsensitive)!=0){result.error=QStringLiteral("The local path resolves to another spelling or target. Select its explicit native path.");return result;}
        if(elapsed.elapsed()>5000){result.error=QStringLiteral("Local file verification exceeded its read budget.");return result;}
    }
    HANDLE file=guard->handles.back();LARGE_INTEGER length{};
    if(!GetFileSizeEx(file,&length)||length.QuadPart!=bytes){result.error=QStringLiteral("The Windows file size differs from the scoped WSL snapshot.");return result;}
    QCryptographicHash hash(QCryptographicHash::Sha256);qint64 read=0;
    char buffer[65536];
    while(read<bytes){
        DWORD count=0;
        if(!ReadFile(file,buffer,DWORD(qMin<qint64>(sizeof buffer,bytes-read)),&count,nullptr)||!count){result.error=QStringLiteral("The Windows file could not be read completely.");return result;}
        hash.addData(QByteArrayView(buffer,count));read+=count;
        if(elapsed.elapsed()>5000){result.error=QStringLiteral("Local file verification exceeded its read budget.");return result;}
    }
    if(hash.result().toHex()!=sha256.toLatin1()){result.error=QStringLiteral("The Windows and WSL files have different contents. Nothing was opened.");return result;}
    result.path=path;result.guard=std::move(guard);return result;
#endif
}
}
