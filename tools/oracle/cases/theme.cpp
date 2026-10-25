// theme: Omareel::Theme's properties for every Omarchy theme and the crafted theme states.

#include "common.h"

#include "core/Theme.h"

#include <QDir>
#include <QFile>
#include <QMetaObject>
#include <QMetaProperty>
#include <QVariant>

namespace Oracle {

namespace {

QByteArray variantText(const QVariant &value)
{
    switch (value.metaType().id()) {
    case QMetaType::QColor: return colorText(value.value<QColor>());
    case QMetaType::QString: return quote(value.toString());
    case QMetaType::Int: return QByteArray::number(value.toInt());
    case QMetaType::Bool: return boolText(value.toBool());
    case QMetaType::QVariantMap: {
        const QVariantMap map = value.toMap();
        QList<QByteArray> items;
        for (auto it = map.begin(); it != map.end(); ++it)
            items << it.key().toUtf8() + '=' + variantText(it.value());
        return '{' + items.join(',') + '}';
    }
    default: return "<" + QByteArray(value.typeName()) + "> " + quote(value.toString());
    }
}

void dumpTheme(Text &out, const Omareel::Theme &theme)
{
    const QMetaObject *meta = theme.metaObject();
    for (int i = meta->propertyOffset(); i < meta->propertyCount(); ++i) {
        const QMetaProperty property = meta->property(i);
        out.row({property.name(), variantText(property.read(&theme))});
    }
    for (const bool focused : {false, true})
        for (const bool hot : {false, true})
            for (const bool selected : {false, true}) {
                const QByteArray arguments = '(' + boolText(focused) + ',' + boolText(hot) + ','
                    + boolText(selected) + ')';
                out.row({"controlFill" + arguments, colorText(theme.controlFill(focused, hot, selected))});
                out.row({"controlBorder" + arguments, colorText(theme.controlBorder(focused, hot, selected))});
                out.row({"controlBorderWidth" + arguments,
                         QByteArray::number(theme.controlBorderWidth(focused, hot, selected))});
            }
}

// A state directory as Omarchy lays it out: theme.name next to theme/colors.toml and
// theme/shell.toml. Files missing from the corpus case stay missing.
QString makeState(const Context &context, const QString &group, const QString &name, const QString &source)
{
    const QString state = QDir(context.scratch).filePath(group + QLatin1Char('/') + name);
    QDir(state).removeRecursively();
    QDir().mkpath(state);
    for (const auto &[from, to] : QList<QPair<QString, QString>>{
             {QStringLiteral("colors.toml"), QStringLiteral("theme/colors.toml")},
             {QStringLiteral("shell.toml"), QStringLiteral("theme/shell.toml")},
             {QStringLiteral("theme.name"), QStringLiteral("theme.name")}}) {
        bool ok = false;
        const QByteArray data = readFile(QDir(source).filePath(from), &ok);
        if (ok) writeFile(QDir(state).filePath(to), data);
    }
    return state;
}

struct Environment {
    QByteArray label;
    QList<QPair<const char *, QByteArray>> set;
};

const char *const Variables[] = {"OMAREEL_THEME_RADIUS", "OMAREEL_ORACLE_HYPRCTL_OUTPUT",
                                 "OMAREEL_ORACLE_HYPRCTL_EXIT", "OMAREEL_ORACLE_FC_MATCH",
                                 "OMAREEL_ORACLE_FC_MATCH_EXIT"};

void applyEnvironment(const Environment &environment)
{
    for (const char *name : Variables) qunsetenv(name);
    for (const auto &[name, value] : environment.set) qputenv(name, value);
}

} // namespace

void runTheme(const Context &context)
{
    applyEnvironment({});
    Text out;
    out.row({"bundledUiFamily()", quote(Omareel::Theme::bundledUiFamily())});

    const auto runCase = [&](const QByteArray &label, const QString &state, const Environment &environment) {
        qputenv("OMAREEL_OMARCHY_STATE_DIR", state.toUtf8());
        applyEnvironment(environment);
        Omareel::Theme theme;
        out << "== " << label << "\n";
        if (!environment.label.isEmpty()) out.row({"environment", environment.label});
        dumpTheme(out, theme);
        applyEnvironment({});
    };

    for (const char *group : {"themes", "themes-crafted"}) {
        const QString root = QDir(context.corpus).filePath(QLatin1String(group));
        for (const QString &name : sortedEntries(root, true)) {
            const QString state = makeState(context, QLatin1String(group), name, QDir(root).filePath(name));
            runCase(QByteArray(group) + '/' + name.toUtf8(), state, {});
        }
    }
    const QString base = makeState(context, QStringLiteral("variants"), QStringLiteral("tokyo-night"),
                                   QDir(context.corpus).filePath(QStringLiteral("themes/tokyo-night")));
    const QList<Environment> variants{
        {"OMAREEL_THEME_RADIUS=0", {{"OMAREEL_THEME_RADIUS", "0"}}},
        {"OMAREEL_THEME_RADIUS=1", {{"OMAREEL_THEME_RADIUS", "1"}}},
        {"OMAREEL_THEME_RADIUS=4", {{"OMAREEL_THEME_RADIUS", "4"}}},
        {"OMAREEL_THEME_RADIUS=7", {{"OMAREEL_THEME_RADIUS", "7"}}},
        {"OMAREEL_THEME_RADIUS=8", {{"OMAREEL_THEME_RADIUS", "8"}}},
        {"OMAREEL_THEME_RADIUS=13", {{"OMAREEL_THEME_RADIUS", "13"}}},
        {"OMAREEL_THEME_RADIUS=24", {{"OMAREEL_THEME_RADIUS", "24"}}},
        {"OMAREEL_THEME_RADIUS=-3", {{"OMAREEL_THEME_RADIUS", "-3"}}},
        {"OMAREEL_THEME_RADIUS=0x10", {{"OMAREEL_THEME_RADIUS", "0x10"}}},
        {"OMAREEL_THEME_RADIUS=010", {{"OMAREEL_THEME_RADIUS", "010"}}},
        {"OMAREEL_THEME_RADIUS=' 12'", {{"OMAREEL_THEME_RADIUS", " 12"}}},
        {"OMAREEL_THEME_RADIUS='12 '", {{"OMAREEL_THEME_RADIUS", "12 "}}},
        {"OMAREEL_THEME_RADIUS=+5", {{"OMAREEL_THEME_RADIUS", "+5"}}},
        {"OMAREEL_THEME_RADIUS=abc", {{"OMAREEL_THEME_RADIUS", "abc"}}},
        {"OMAREEL_THEME_RADIUS=''", {{"OMAREEL_THEME_RADIUS", ""}}},
        {"OMAREEL_THEME_RADIUS=4.5", {{"OMAREEL_THEME_RADIUS", "4.5"}}},
        {"OMAREEL_THEME_RADIUS=99999999999", {{"OMAREEL_THEME_RADIUS", "99999999999"}}},
        {"hyprctl int 0", {{"OMAREEL_ORACLE_HYPRCTL_OUTPUT", R"({"option": "decoration:rounding", "int": 0, "set": true })"}}},
        {"hyprctl int 2", {{"OMAREEL_ORACLE_HYPRCTL_OUTPUT", R"({"option": "decoration:rounding", "int": 2, "set": true })"}}},
        {"hyprctl int 5", {{"OMAREEL_ORACLE_HYPRCTL_OUTPUT", R"({"option": "decoration:rounding", "int": 5, "set": true })"}}},
        {"hyprctl int 13", {{"OMAREEL_ORACLE_HYPRCTL_OUTPUT", R"({"option": "decoration:rounding", "int": 13, "set": true })"}}},
        {"hyprctl int 20", {{"OMAREEL_ORACLE_HYPRCTL_OUTPUT", R"({"option": "decoration:rounding", "int": 20, "set": true })"}}},
        {"hyprctl int -4", {{"OMAREEL_ORACLE_HYPRCTL_OUTPUT", R"({"option": "decoration:rounding", "int": -4, "set": true })"}}},
        {"hyprctl int 12.5", {{"OMAREEL_ORACLE_HYPRCTL_OUTPUT", R"({"int":12.5})"}}},
        {"hyprctl compact int 6", {{"OMAREEL_ORACLE_HYPRCTL_OUTPUT", R"({"int":6})"}}},
        {"hyprctl int 99999999999", {{"OMAREEL_ORACLE_HYPRCTL_OUTPUT", R"({"int": 99999999999})"}}},
        {"hyprctl two ints", {{"OMAREEL_ORACLE_HYPRCTL_OUTPUT", R"({"int": 3, "int": 9})"}}},
        {"hyprctl no int", {{"OMAREEL_ORACLE_HYPRCTL_OUTPUT", R"({"option": "decoration:rounding", "float": 4.0})"}}},
        {"hyprctl not JSON", {{"OMAREEL_ORACLE_HYPRCTL_OUTPUT", "no such option"}}},
        {"hyprctl empty output", {{"OMAREEL_ORACLE_HYPRCTL_OUTPUT", ""}}},
        {"hyprctl exits 1 after printing int 7",
         {{"OMAREEL_ORACLE_HYPRCTL_OUTPUT", R"({"int": 7})"}, {"OMAREEL_ORACLE_HYPRCTL_EXIT", "1"}}},
        {"hyprctl int 7 and OMAREEL_THEME_RADIUS=3",
         {{"OMAREEL_ORACLE_HYPRCTL_OUTPUT", R"({"int": 7})"}, {"OMAREEL_THEME_RADIUS", "3"}}},
        {"fc-match JetBrainsMono Nerd Font", {{"OMAREEL_ORACLE_FC_MATCH", "JetBrainsMono Nerd Font"}}},
        {"fc-match with surrounding whitespace", {{"OMAREEL_ORACLE_FC_MATCH", "  Spaced Mono \n"}}},
        {"fc-match empty output", {{"OMAREEL_ORACLE_FC_MATCH", ""}}},
        {"fc-match exits 1", {{"OMAREEL_ORACLE_FC_MATCH_EXIT", "1"}}},
        {"fc-match non-ASCII", {{"OMAREEL_ORACLE_FC_MATCH", "M\xc3\xb6no \xe2\x84\x96" "1"}}},
    };
    for (const Environment &environment : variants)
        runCase("variant tokyo-night " + environment.label, base, environment);

    // setFontFamilies() replaces the interface family for later Themes; the mono family argument
    // is ignored. An empty family restores the default.
    Omareel::Theme::setFontFamilies(QStringLiteral("Configured Sans"), QStringLiteral("Ignored Mono"));
    runCase("variant tokyo-night setFontFamilies(\"Configured Sans\", \"Ignored Mono\")", base, {});
    Omareel::Theme::setFontFamilies(QString(), QString());
    runCase("variant tokyo-night setFontFamilies(\"\", \"\")", base, {});

    // reload() re-reads the files but keeps the radius when hyprctl stops answering.
    {
        const QString state = makeState(context, QStringLiteral("reload"), QStringLiteral("tokyo-night"),
                                        QDir(context.corpus).filePath(QStringLiteral("themes/tokyo-night")));
        qputenv("OMAREEL_OMARCHY_STATE_DIR", state.toUtf8());
        applyEnvironment({"", {{"OMAREEL_ORACLE_HYPRCTL_OUTPUT", R"({"int": 14})"}}});
        Omareel::Theme theme;
        out << "== reload: constructed with tokyo-night and hyprctl int 14\n";
        dumpTheme(out, theme);
        const QString other = QDir(context.corpus).filePath(QStringLiteral("themes/catppuccin-latte"));
        writeFile(QDir(state).filePath(QStringLiteral("theme/colors.toml")), readFile(QDir(other).filePath(QStringLiteral("colors.toml"))));
        writeFile(QDir(state).filePath(QStringLiteral("theme/shell.toml")), readFile(QDir(other).filePath(QStringLiteral("shell.toml"))));
        writeFile(QDir(state).filePath(QStringLiteral("theme.name")), "catppuccin-latte\n");
        applyEnvironment({"", {{"OMAREEL_ORACLE_HYPRCTL_EXIT", "1"}, {"OMAREEL_ORACLE_HYPRCTL_OUTPUT", ""}}});
        QMetaObject::invokeMethod(&theme, "reload", Qt::DirectConnection);
        out << "== reload: files replaced by catppuccin-latte, hyprctl fails\n";
        dumpTheme(out, theme);
        applyEnvironment({"", {{"OMAREEL_THEME_RADIUS", "-9"}}});
        QMetaObject::invokeMethod(&theme, "reload", Qt::DirectConnection);
        out << "== reload: OMAREEL_THEME_RADIUS=-9\n";
        dumpTheme(out, theme);
        applyEnvironment({});
        QDir(state).removeRecursively();
        QMetaObject::invokeMethod(&theme, "reload", Qt::DirectConnection);
        out << "== reload: state directory removed, default hyprctl\n";
        dumpTheme(out, theme);
    }
    qunsetenv("OMAREEL_OMARCHY_STATE_DIR");
    applyEnvironment({});
    writeFile(QDir(context.output).filePath(QStringLiteral("cases.txt")), out.data());
}

} // namespace Oracle
