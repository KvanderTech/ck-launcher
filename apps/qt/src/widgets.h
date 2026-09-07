#pragma once
#include <QJsonObject>
#include <QtWidgets>
#include <functional>
inline QString s(const char *text) {
    return QString::fromLatin1(text);
}
inline QString value(const QJsonObject &object, const char *key) {
    return object.value(s(key)).toString();
}
inline QPushButton *button(const QString &title, QBoxLayout *row,
                           const std::function<void()> &action, QObject *context,
                           bool primary = false) {
    auto *b = new QPushButton(title);
    if (primary)
        b->setProperty("primary", true);
    b->setCursor(Qt::PointingHandCursor);
    row->addWidget(b);
    QObject::connect(b, &QPushButton::clicked, context, action);
    return b;
}
inline QTableWidget *table(const QStringList &labels, QVBoxLayout *layout) {
    auto *t = new QTableWidget(0, labels.size());
    t->setHorizontalHeaderLabels(labels);
    t->setSelectionBehavior(QAbstractItemView::SelectRows);
    t->setSelectionMode(QAbstractItemView::SingleSelection);
    t->setEditTriggers(QAbstractItemView::NoEditTriggers);
    t->verticalHeader()->hide();
    t->horizontalHeader()->setStretchLastSection(true);
    t->horizontalHeader()->setSectionResizeMode(QHeaderView::ResizeToContents);
    t->setAlternatingRowColors(true);
    t->setShowGrid(false);
    layout->addWidget(t, 1);
    return t;
}
inline void cells(QTableWidget *table, int row, const QStringList &values) {
    for (int i = 0; i < values.size(); ++i)
        table->setItem(row, i, new QTableWidgetItem(values[i]));
}
inline QVBoxLayout *pageLayout(QWidget *page, const QString &title, const QString &subtitle) {
    auto *layout = new QVBoxLayout(page);
    layout->setContentsMargins(30, 24, 30, 16);
    layout->setSpacing(14);
    auto *heading = new QLabel(title);
    heading->setProperty("heading", true);
    layout->addWidget(heading);
    auto *description = new QLabel(subtitle);
    description->setWordWrap(true);
    description->setProperty("muted", true);
    layout->addWidget(description);
    return layout;
}
