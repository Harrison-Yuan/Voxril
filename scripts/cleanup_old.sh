import shutil, os

workspace = '/Users/harrison/Desktop/my_voice_training'

dirs_to_delete = ['data', 'models']

for d in dirs_to_delete:
    path = os.path.join(workspace, d)
    if os.path.exists(path):
        shutil.rmtree(path)
        print(f'deleted {d}/')
    else:
        print(f'{d}/ not found, skip')

print('done')
