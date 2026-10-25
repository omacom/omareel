// omarchypaths: every function in OmarchyPaths.h over a fake Omarchy tree in the hermetic HOME.
//
// themeBackgroundGroups() also scans the hard-coded /usr/share/omarchy/themes, which depends on
// the machine. Its paths are dropped from the dumps here (see README), which is exact because
// that root is scanned last and the fake tree never links into it.

#include "common.h"

#include "core/OmarchyPaths.h"

#include <QCryptographicHash>
#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QImage>
#include <QImageReader>
#include <QPainter>
#include <QVariant>

#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>

namespace Oracle {

namespace OP = Omareel::OmarchyPaths;

namespace {

QString home()
{
    return QDir::homePath();
}

QString inHome(const QString &relative)
{
    return QDir(home()).filePath(relative);
}

void touch(const QString &path, const QByteArray &data = QByteArray("image"))
{
    writeFile(path, data);
}

void symlink(const QString &target, const QString &link)
{
    QDir().mkpath(QFileInfo(link).absolutePath());
    QFile::remove(link);
    if (::symlink(QFile::encodeName(target).constData(), QFile::encodeName(link).constData()) != 0)
        fail(QStringLiteral("symlink %1 -> %2 failed").arg(link, target));
}

void removePath(const QString &path)
{
    const QFileInfo info(path);
    if (info.isSymLink() || info.isFile()) QFile::remove(path);
    else if (info.isDir()) QDir(path).removeRecursively();
}

bool hermetic(const QString &path)
{
    return sanitize(path).startsWith(QLatin1Char('$'));
}

void buildTree()
{
    const QString config = inHome(QStringLiteral(".config/omarchy/themes"));
    const QString share = inHome(QStringLiteral(".local/share/omarchy/themes"));
    for (const char *name : {"b.png", "A.PNG", "c.JPEG", "d.webp", "e.Jpg", ".hidden.png", "notes.txt",
                             "f.gif", "g.png.bak", "\xc3\xbcn\xc3\xaf.png", "space name.jpg"})
        touch(QDir(config).filePath(QStringLiteral("alpha/backgrounds/") + QString::fromUtf8(name)));
    QDir().mkpath(QDir(config).filePath(QStringLiteral("alpha/backgrounds/sub.png")));
    symlink(QStringLiteral("../../beta/backgrounds/x.jpg"), QDir(config).filePath(QStringLiteral("alpha/backgrounds/link-to-beta.jpg")));
    symlink(QStringLiteral("nowhere.png"), QDir(config).filePath(QStringLiteral("alpha/backgrounds/broken.png")));
    for (const char *name : {"x.jpg", "y.jpeg", "Z.png", "a b.png"})
        touch(QDir(config).filePath(QStringLiteral("beta/backgrounds/") + QLatin1String(name)));
    QDir().mkpath(QDir(config).filePath(QStringLiteral("empty-theme")));
    touch(QDir(config).filePath(QStringLiteral("backgrounds-file/backgrounds")));
    touch(QDir(config).filePath(QStringLiteral(".hidden-theme/backgrounds/h.png")));
    symlink(QStringLiteral("beta"), QDir(config).filePath(QStringLiteral("linked-beta")));
    touch(QDir(config).filePath(QStringLiteral("my-multi-word-theme/backgrounds/m.jpg")));
    touch(QDir(config).filePath(QStringLiteral("Upper/backgrounds/u.png")));
    touch(QDir(config).filePath(QStringLiteral("plain-file")));
    touch(QDir(config).filePath(QStringLiteral("-dash-first/backgrounds/d.png")));
    touch(QDir(share).filePath(QStringLiteral("alpha/backgrounds/b.png")), "another image");
    touch(QDir(share).filePath(QStringLiteral("alpha/backgrounds/s.png")));
    touch(QDir(share).filePath(QStringLiteral("gamma/backgrounds/g.png")));
    touch(QDir(share).filePath(QString::fromUtf8("\xc3\xbcn\xc3\xaf" "code-th\xc3\xa8me/backgrounds/u.webp")));
    symlink(QDir(config).filePath(QStringLiteral("alpha/backgrounds/b.png")),
            QDir(share).filePath(QStringLiteral("zeta/backgrounds/z.jpg")));
    touch(inHome(QStringLiteral("Pictures/outside.png")));

    const QString state = inHome(QStringLiteral(".local/state/omarchy/current"));
    writeFile(QDir(state).filePath(QStringLiteral("theme.name")), "beta\n");
    for (const char *name : {"current-b.png", "Current-A.jpg", ".hidden.png", "readme.txt"})
        touch(QDir(state).filePath(QStringLiteral("theme/backgrounds/") + QLatin1String(name)));
    symlink(QDir(config).filePath(QStringLiteral("beta/backgrounds/y.jpeg")), QDir(state).filePath(QStringLiteral("background")));

    const QString alternative = inHome(QStringLiteral("alt-state"));
    writeFile(QDir(alternative).filePath(QStringLiteral("theme.name")), "gamma");
    symlink(QDir(share).filePath(QStringLiteral("gamma/backgrounds/g.png")), QDir(alternative).filePath(QStringLiteral("background")));
}

void dumpState(Text &out, const QByteArray &label)
{
    out << "== " << label << "\n";
    out.row({"stateRoot", quote(sanitize(OP::stateRoot()))});
    out.row({"currentThemeName", quote(OP::currentThemeName())});
    out.row({"currentBackground", quote(sanitize(OP::currentBackground()))});
    for (const QVariant &groupValue : OP::themeBackgroundGroups()) {
        const QVariantMap group = groupValue.toMap();
        QList<QByteArray> paths;
        for (const QVariant &path : group.value(QStringLiteral("paths")).toList())
            if (hermetic(path.toString())) paths << quote(sanitize(path.toString()));
        if (paths.isEmpty()) continue;
        QStringList keys = group.keys();
        out.row({"group", quote(group.value(QStringLiteral("name")).toString()),
                 quote(group.value(QStringLiteral("label")).toString()),
                 boolText(group.value(QStringLiteral("current")).toBool()),
                 QByteArray(group.value(QStringLiteral("current")).typeName()), quoteList(keys)});
        for (const QByteArray &path : paths) out.row({"  path", path});
    }
    for (const QString &path : OP::themeBackgrounds())
        if (hermetic(path)) out.row({"themeBackgrounds", quote(sanitize(path))});
}

void setEnv(const char *name, const QString &value)
{
    qputenv(name, value.toUtf8());
}

QByteArray identityOf(const QString &path)
{
    const QFileInfo source(path);
    return source.absoluteFilePath().toUtf8() + '\0' + QByteArray::number(source.size()) + '\0'
        + QByteArray::number(source.lastModified().toMSecsSinceEpoch());
}

void dumpThumbnailPath(Text &out, const QByteArray &label, const QString &input)
{
    const QString result = OP::wallpaperThumbnailPath(input);
    const QByteArray identity = identityOf(input);
    const QString hash = QString::fromLatin1(QCryptographicHash::hash(identity, QCryptographicHash::Sha256).toHex());
    const bool matches = result.endsWith(QLatin1Char('/') + hash + QStringLiteral(".jpg"));
    const QString identityText = QString::fromUtf8(identity);
    QString shown = sanitize(result);
    // A hash over a path inside the scratch HOME changes with the machine; show how it is made.
    if (sanitize(identityText) != identityText)
        shown.replace(hash, QStringLiteral("{sha256(identity)}"));
    out.row({label, quote(sanitize(input)),
             quote(sanitize(identityText)), matches ? "hash=sha256(identity)" : "hash-differs",
             quote(shown)});
}

void setTimes(const QString &path, qint64 seconds, long nanoseconds)
{
    const struct timespec times[2] = {{seconds, nanoseconds}, {seconds, nanoseconds}};
    if (::utimensat(AT_FDCWD, QFile::encodeName(path).constData(), times, AT_SYMLINK_NOFOLLOW) != 0)
        fail(QStringLiteral("utimensat %1").arg(path));
}

QImage pattern(int width, int height)
{
    QImage image(width, height, QImage::Format_RGB32);
    for (int y = 0; y < height; ++y)
        for (int x = 0; x < width; ++x)
            image.setPixel(x, y, qRgb(x * 255 / qMax(1, width - 1), y * 255 / qMax(1, height - 1), (x ^ y) & 0xff));
    return image;
}

} // namespace

void runOmarchyPaths(const Context &context)
{
    qunsetenv("OMAREEL_OMARCHY_STATE_DIR");
    qunsetenv("OMAREEL_RECENTS_DIR");
    const QString xdgCache = qEnvironmentVariable("XDG_CACHE_HOME");
    buildTree();
    const QString state = inHome(QStringLiteral(".local/state/omarchy/current"));
    const QString config = inHome(QStringLiteral(".config/omarchy/themes"));
    const QString share = inHome(QStringLiteral(".local/share/omarchy/themes"));

    Text paths;
    dumpState(paths, "default state: background links into a theme folder");
    symlink(inHome(QStringLiteral("Pictures/outside.png")), QDir(state).filePath(QStringLiteral("background")));
    dumpState(paths, "background links outside the theme folders");
    QFile::remove(QDir(state).filePath(QStringLiteral("theme.name")));
    dumpState(paths, "theme.name missing");
    writeFile(QDir(state).filePath(QStringLiteral("theme.name")), "empty-theme");
    dumpState(paths, "theme.name names a theme without backgrounds");
    writeFile(QDir(state).filePath(QStringLiteral("theme.name")), " beta \r\n\r\n");
    dumpState(paths, "theme.name with whitespace and CRLF");
    writeFile(QDir(state).filePath(QStringLiteral("theme.name")), "Upper\n");
    dumpState(paths, "theme.name with an upper-case theme");
    writeFile(QDir(state).filePath(QStringLiteral("theme.name")), "\xc3\xbcn\xc3\xaf" "code-th\xc3\xa8me\n");
    symlink(QDir(share).filePath(QString::fromUtf8("\xc3\xbcn\xc3\xaf" "code-th\xc3\xa8me/backgrounds/u.webp")),
            QDir(state).filePath(QStringLiteral("background")));
    dumpState(paths, "theme.name with non-ASCII letters");
    writeFile(QDir(state).filePath(QStringLiteral("theme.name")), " \t\n");
    dumpState(paths, "theme.name only whitespace");
    writeFile(QDir(state).filePath(QStringLiteral("theme.name")), "beta\n");
    symlink(QStringLiteral("missing.png"), QDir(state).filePath(QStringLiteral("background")));
    dumpState(paths, "background link is broken");
    symlink(QStringLiteral("theme/backgrounds/current-b.png"), QDir(state).filePath(QStringLiteral("background")));
    dumpState(paths, "background link is relative");
    symlink(QStringLiteral("theme/backgrounds"), QDir(state).filePath(QStringLiteral("background")));
    dumpState(paths, "background links to a directory");
    symlink(QStringLiteral("link-two"), QDir(state).filePath(QStringLiteral("background")));
    symlink(QDir(config).filePath(QStringLiteral("beta/backgrounds/x.jpg")), QDir(state).filePath(QStringLiteral("link-two")));
    dumpState(paths, "background is a chain of links");
    QFile::remove(QDir(state).filePath(QStringLiteral("background")));
    touch(QDir(state).filePath(QStringLiteral("background")));
    dumpState(paths, "background is a regular file");
    QFile::remove(QDir(state).filePath(QStringLiteral("background")));
    dumpState(paths, "no background link");
    QDir(QDir(state).filePath(QStringLiteral("theme"))).removeRecursively();
    dumpState(paths, "no background link and no theme/backgrounds");
    setEnv("OMAREEL_OMARCHY_STATE_DIR", inHome(QStringLiteral("alt-state")));
    dumpState(paths, "OMAREEL_OMARCHY_STATE_DIR absolute");
    setEnv("OMAREEL_OMARCHY_STATE_DIR", inHome(QStringLiteral("alt-state/./")));
    dumpState(paths, "OMAREEL_OMARCHY_STATE_DIR with ./ and a trailing slash");
    setEnv("OMAREEL_OMARCHY_STATE_DIR", inHome(QStringLiteral("Pictures/../alt-state")));
    dumpState(paths, "OMAREEL_OMARCHY_STATE_DIR with ..");
    setEnv("OMAREEL_OMARCHY_STATE_DIR", QStringLiteral("alt-state"));
    dumpState(paths, "OMAREEL_OMARCHY_STATE_DIR relative (working directory $HOME)");
    setEnv("OMAREEL_OMARCHY_STATE_DIR", inHome(QStringLiteral("nowhere")));
    dumpState(paths, "OMAREEL_OMARCHY_STATE_DIR missing");
    setEnv("OMAREEL_OMARCHY_STATE_DIR", QString());
    dumpState(paths, "OMAREEL_OMARCHY_STATE_DIR empty");
    qunsetenv("OMAREEL_OMARCHY_STATE_DIR");
    symlink(inHome(QStringLiteral("Pictures/outside.png")), QDir(state).filePath(QStringLiteral("background")));
    QDir().rename(inHome(QStringLiteral(".config/omarchy")), inHome(QStringLiteral(".config/omarchy-away")));
    QDir().rename(inHome(QStringLiteral(".local/share/omarchy")), inHome(QStringLiteral(".local/share/omarchy-away")));
    dumpState(paths, "no theme folders");
    QFile::remove(QDir(state).filePath(QStringLiteral("theme.name")));
    dumpState(paths, "no theme folders and no theme.name");
    writeFile(QDir(context.output).filePath(QStringLiteral("state.txt")), paths.data());

    // recordingsDirectory()
    Text recordings;
    const auto recordingsRow = [&recordings](const QByteArray &label) {
        recordings.row({label, quote(sanitize(OP::recordingsDirectory()))});
    };
    const QString xdgConfig = qEnvironmentVariable("XDG_CONFIG_HOME");
    const QString userDirs = QDir(xdgConfig).filePath(QStringLiteral("user-dirs.dirs"));
    for (const auto &[label, value] : QList<QPair<QByteArray, QString>>{
             {"OMAREEL_RECENTS_DIR=/srv/recents", QStringLiteral("/srv/recents")},
             {"OMAREEL_RECENTS_DIR=/srv/x/../recents/", QStringLiteral("/srv/x/../recents/")},
             {"OMAREEL_RECENTS_DIR=relative/recents", QStringLiteral("relative/recents")},
             {"OMAREEL_RECENTS_DIR=~/recents", QStringLiteral("~/recents")},
             {"OMAREEL_RECENTS_DIR=' '", QStringLiteral(" ")},
             {"OMAREEL_RECENTS_DIR=$HOME/with space/", inHome(QStringLiteral("with space/"))},
             {"OMAREEL_RECENTS_DIR=//double//slashes", QStringLiteral("//double//slashes")}}) {
        setEnv("OMAREEL_RECENTS_DIR", value);
        recordingsRow(label);
    }
    setEnv("OMAREEL_RECENTS_DIR", QString());
    recordingsRow("OMAREEL_RECENTS_DIR='' and no user-dirs.dirs");
    qunsetenv("OMAREEL_RECENTS_DIR");
    recordingsRow("no user-dirs.dirs");
    for (const auto &[label, content] : QList<QPair<QByteArray, QByteArray>>{
             {"user-dirs.dirs $HOME/Movies", "XDG_VIDEOS_DIR=\"$HOME/Movies\"\n"},
             {"user-dirs.dirs absolute", "XDG_VIDEOS_DIR=\"/srv/videos\"\n"},
             {"user-dirs.dirs unquoted", "XDG_VIDEOS_DIR=$HOME/Unquoted\n"},
             {"user-dirs.dirs $HOME alone", "XDG_VIDEOS_DIR=\"$HOME\"\n"},
             {"user-dirs.dirs $HOME/", "XDG_VIDEOS_DIR=\"$HOME/\"\n"},
             {"user-dirs.dirs relative", "XDG_VIDEOS_DIR=\"relative/videos\"\n"},
             {"user-dirs.dirs with spaces", "XDG_VIDEOS_DIR=\"$HOME/With Space/Vids\"\n"},
             {"user-dirs.dirs ${HOME}", "XDG_VIDEOS_DIR=\"${HOME}/Braces\"\n"},
             {"user-dirs.dirs trailing slash", "XDG_VIDEOS_DIR=\"$HOME/Movies/\"\n"},
             {"user-dirs.dirs comments and other keys",
              "# written by xdg-user-dirs-update\nXDG_DESKTOP_DIR=\"$HOME/Desktop\"\n"
              "XDG_VIDEOS_DIR=\"$HOME/Clips\"\nXDG_MUSIC_DIR=\"$HOME/Music\"\n"},
             {"user-dirs.dirs spaces around =", "XDG_VIDEOS_DIR = \"$HOME/Spaced\"\n"},
             {"user-dirs.dirs indented", "   XDG_VIDEOS_DIR=\"$HOME/Indented\"\n"},
             {"user-dirs.dirs twice", "XDG_VIDEOS_DIR=\"$HOME/First\"\nXDG_VIDEOS_DIR=\"$HOME/Second\"\n"},
             {"user-dirs.dirs CRLF", "XDG_VIDEOS_DIR=\"$HOME/Crlf\"\r\n"},
             {"user-dirs.dirs no newline", "XDG_VIDEOS_DIR=\"$HOME/NoNewline\""},
             {"user-dirs.dirs empty value", "XDG_VIDEOS_DIR=\"\"\n"},
             {"user-dirs.dirs non-ASCII", "XDG_VIDEOS_DIR=\"$HOME/Vid\xc3\xa9os\"\n"},
             {"user-dirs.dirs escaped quote", "XDG_VIDEOS_DIR=\"$HOME/a\\\"b\"\n"},
             {"user-dirs.dirs lower-case key", "xdg_videos_dir=\"$HOME/Lower\"\n"},
             {"user-dirs.dirs empty file", ""}}) {
        writeFile(userDirs, content);
        recordingsRow(label);
    }
    QFile::remove(userDirs);
    setEnv("XDG_VIDEOS_DIR", QStringLiteral("/env/videos"));
    recordingsRow("XDG_VIDEOS_DIR=/env/videos in the environment, no user-dirs.dirs");
    qunsetenv("XDG_VIDEOS_DIR");
    writeFile(inHome(QStringLiteral(".config/user-dirs.dirs")), "XDG_VIDEOS_DIR=\"$HOME/FromHomeConfig\"\n");
    qunsetenv("XDG_CONFIG_HOME");
    recordingsRow("XDG_CONFIG_HOME unset, $HOME/.config/user-dirs.dirs");
    writeFile(inHome(QStringLiteral("relconfig/user-dirs.dirs")), "XDG_VIDEOS_DIR=\"$HOME/FromRelative\"\n");
    setEnv("XDG_CONFIG_HOME", QStringLiteral("relconfig"));
    recordingsRow("XDG_CONFIG_HOME=relconfig (relative)");
    setEnv("XDG_CONFIG_HOME", xdgConfig);
    QFile::remove(inHome(QStringLiteral(".config/user-dirs.dirs")));
    writeFile(QDir(context.output).filePath(QStringLiteral("recordings-directory.txt")), recordings.data());

    // wallpaperThumbnailPath()
    Text thumbnails;
    const QString wall = inHome(QStringLiteral("walls/wall.png"));
    writeFile(wall, QByteArray(12345, 'w'));
    setTimes(wall, 1700000000, 123456789);
    const QString wallLink = inHome(QStringLiteral("walls/link.png"));
    symlink(QStringLiteral("wall.png"), wallLink);
    setTimes(wallLink, 1600000000, 0);
    const QString empty = inHome(QStringLiteral("walls/empty.jpg"));
    writeFile(empty, QByteArray());
    setTimes(empty, 0, 0);
    const QString old = inHome(QStringLiteral("walls/before-1970.jpg"));
    writeFile(old, "x");
    setTimes(old, -86400, 500000000);
    for (const auto &[label, input] : QList<QPair<QByteArray, QString>>{
             {"missing absolute", QStringLiteral("/nonexistent/oracle/wall.png")},
             {"missing absolute with spaces and non-ASCII", QStringLiteral(u"/nonexistent/a b/ü\U0001F600.jpg")},
             {"missing with dot segments", QStringLiteral("/nonexistent/x/../y/./wall.png")},
             {"missing with trailing slash", QStringLiteral("/nonexistent/dir/")},
             {"empty", QString()},
             {"existing", wall},
             {"symlink", wallLink},
             {"empty file at the epoch", empty},
             {"before 1970", old},
             {"relative (working directory $HOME)", QStringLiteral("walls/wall.png")}})
        dumpThumbnailPath(thumbnails, label, input);
    qunsetenv("XDG_CACHE_HOME");
    dumpThumbnailPath(thumbnails, "missing absolute, XDG_CACHE_HOME unset", QStringLiteral("/nonexistent/oracle/wall.png"));
    setEnv("XDG_CACHE_HOME", QStringLiteral("relative-cache"));
    dumpThumbnailPath(thumbnails, "missing absolute, XDG_CACHE_HOME relative", QStringLiteral("/nonexistent/oracle/wall.png"));
    setEnv("XDG_CACHE_HOME", xdgCache + QStringLiteral("/./sub/.."));
    dumpThumbnailPath(thumbnails, "missing absolute, XDG_CACHE_HOME=$XDG_CACHE_HOME/./sub/..", QStringLiteral("/nonexistent/oracle/wall.png"));
    writeFile(QDir(context.output).filePath(QStringLiteral("wallpaper-thumbnail-path.txt")), thumbnails.data());

    // generateWallpaperThumbnail(): the JPEG bytes depend on libjpeg, so only the outcome and the
    // image size are recorded.
    setEnv("XDG_CACHE_HOME", xdgCache);
    Text generate;
    const QString sources = inHome(QStringLiteral("thumbnail-sources"));
    QDir().mkpath(sources);
    pattern(800, 600).save(QDir(sources).filePath(QStringLiteral("landscape.png")));
    pattern(100, 50).save(QDir(sources).filePath(QStringLiteral("small.png")));
    pattern(300, 1000).save(QDir(sources).filePath(QStringLiteral("tall.png")));
    pattern(400, 240).save(QDir(sources).filePath(QStringLiteral("exact.png")));
    pattern(401, 241).save(QDir(sources).filePath(QStringLiteral("odd.png")));
    pattern(1, 1).save(QDir(sources).filePath(QStringLiteral("pixel.png")));
    pattern(640, 480).save(QDir(sources).filePath(QStringLiteral("jpeg-named.webp")), "PNG");
    writeFile(QDir(sources).filePath(QStringLiteral("not-an-image.png")), "this is text");
    touch(inHome(QStringLiteral("blocker")));
    const QString thumbs = inHome(QStringLiteral("thumbnails"));
    const auto generateRow = [&](const QByteArray &label, const QString &source, const QString &thumbnail) {
        const bool ok = OP::generateWallpaperThumbnail(source, thumbnail);
        QImageReader reader(thumbnail);
        const QSize size = reader.size();
        generate.row({label, boolText(ok), boolText(QFileInfo(thumbnail).isFile()),
                      size.isValid() ? QByteArray::number(size.width()) + 'x' + QByteArray::number(size.height()) : "-",
                      reader.format().isEmpty() ? "-" : reader.format()});
    };
    for (const char *name : {"landscape.png", "small.png", "tall.png", "exact.png", "odd.png", "pixel.png",
                             "jpeg-named.webp", "not-an-image.png", "missing.png"})
        generateRow(QByteArray(name), QDir(sources).filePath(QLatin1String(name)),
                    QDir(thumbs).filePath(QLatin1String(name) + QStringLiteral(".jpg")));
    generateRow("landscape.png again (thumbnail exists)", QDir(sources).filePath(QStringLiteral("landscape.png")),
                QDir(thumbs).filePath(QStringLiteral("landscape.png.jpg")));
    writeFile(QDir(thumbs).filePath(QStringLiteral("junk.jpg")), "junk");
    generateRow("missing source, thumbnail file exists", QDir(sources).filePath(QStringLiteral("missing.png")),
                QDir(thumbs).filePath(QStringLiteral("junk.jpg")));
    generateRow("thumbnail directory cannot be created", QDir(sources).filePath(QStringLiteral("small.png")),
                inHome(QStringLiteral("blocker/sub/small.jpg")));
    generateRow("into the cache via wallpaperThumbnailPath", QDir(sources).filePath(QStringLiteral("tall.png")),
                OP::wallpaperThumbnailPath(QDir(sources).filePath(QStringLiteral("tall.png"))));
    writeFile(QDir(context.output).filePath(QStringLiteral("generate-wallpaper-thumbnail.txt")), generate.data());

    // Leave the hermetic HOME as the next family expects it.
    QDir(QDir(xdgCache).filePath(QStringLiteral("omareel"))).removeRecursively();
    for (const char *path : {".config", ".local", "alt-state", "Pictures", "walls", "thumbnail-sources",
                             "thumbnails", "blocker", "relconfig", "relative-cache"})
        removePath(inHome(QLatin1String(path)));
}

} // namespace Oracle
