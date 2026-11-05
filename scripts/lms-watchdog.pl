#!/usr/bin/perl
# MusicCraft-owned process wrapper; upstream LMS remains unmodified.
use strict;
use warnings;
use POSIX qw(WNOHANG);
my $owner = shift @ARGV;
die "Invalid owner\n" unless defined($owner) && $owner =~ /^\d+$/ && $owner > 1;
my $child = fork();
die "Cannot start server\n" unless defined($child);
if (!$child) { exec $^X, @ARGV; exit 127; }
my $stopping;
$SIG{TERM} = $SIG{INT} = sub { $stopping //= time(); kill 'TERM', $child; };
while (1) {
    my $done = waitpid($child, WNOHANG);
    last if $done == $child || $done == -1;
    if (!$stopping && !kill(0, $owner)) { $stopping = time(); kill 'TERM', $child; }
    if ($stopping && time() - $stopping >= 5) { kill 'KILL', $child; waitpid($child, 0); last; }
    select undef, undef, undef, 0.2;
}
